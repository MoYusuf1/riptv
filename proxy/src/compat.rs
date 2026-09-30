//! Compatibility mode. Browsers on many machines can't decode HEVC (every 4K channel), AC-3,
//! MP2 or AAC-Main sound, or raw MPEG-TS streams, and no amount of code in the page changes that.
//! If `ffmpeg` is installed, this turns such a stream into H.264 + AAC in fragmented MP4 and
//! pipes it to the page as it goes: the video is copied untouched whenever it is already fine, so
//! a sound-only problem costs almost nothing, and only HEVC (or interlaced) video is re-encoded.
//!
//! ponytail: this is the one place outside Rust, and only when a stream needs it. ffmpeg follows
//! redirects itself, so the public-address check only covers the address the page asked for, not
//! where the provider then sends ffmpeg.

use std::{
    io,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_core::Stream;
use reqwest::Url;
use serde::Deserialize;
use tokio::{
    process::{Child, ChildStdout, Command},
    sync::OnceCell,
    time::timeout,
};
use tokio_util::io::ReaderStream;

use crate::{AppState, FALLBACK_UA, from_app, refusal, url_ok};

const MISSING: &str =
    "ffmpeg isn't installed, and this stream needs it (Arch: sudo pacman -S ffmpeg)";
const VIDEO_HEADER: HeaderName = HeaderName::from_static("x-riptv-video");

/// Only plain network protocols: a playlist from a provider must not be able to make ffmpeg read
/// local files.
const PROTOCOLS: &str = "http,https,tcp,tls,crypto";

#[derive(Deserialize)]
pub struct CompatQuery {
    url: String,
    /// `copy` or `transcode`, as reported by the check; looked up again if absent.
    video: Option<String>,
}

/// What the first video stream is, from ffprobe.
#[derive(Debug, PartialEq)]
struct Probe {
    codec: String,
    pix_fmt: String,
    field_order: String,
    height: u32,
}

impl Probe {
    /// Copying is only right for plain 8-bit progressive H.264, which every browser plays.
    fn can_copy(&self) -> bool {
        self.codec == "h264"
            && self.pix_fmt == "yuv420p"
            && matches!(self.field_order.as_str(), "progressive" | "unknown" | "")
    }

    /// ffprobe's `key=value` lines. `None` if there is no video stream (radio); anything about
    /// it that's missing counts against copying.
    fn parse(text: &str) -> Option<Probe> {
        let get = |key: &str| {
            text.lines()
                .find_map(|l| l.trim().strip_prefix(key)?.strip_prefix('='))
                .unwrap_or("")
                .to_owned()
        };
        let codec = get("codec_name");
        (!codec.is_empty()).then(|| Probe {
            codec,
            pix_fmt: get("pix_fmt"),
            field_order: get("field_order"),
            height: get("height").parse().unwrap_or(0),
        })
    }
}

enum Failure {
    Missing,
    Failed(String),
}

fn not_found(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound
}

async fn probe(url: &str, ua: &str) -> Result<Option<Probe>, Failure> {
    let run = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-user_agent",
            ua,
            "-protocol_whitelist",
            PROTOCOLS,
        ])
        .args([
            "-rw_timeout",
            "15000000",
            "-analyzeduration",
            "3000000",
            "-probesize",
            "5000000",
        ])
        .args(["-select_streams", "v:0", "-show_entries"])
        .arg("stream=codec_name,pix_fmt,field_order,height")
        .args(["-of", "default=noprint_wrappers=1", url])
        .stdin(Stdio::null())
        .kill_on_drop(true)
        .output();
    let out = match timeout(Duration::from_secs(30), run).await {
        Err(_) => return Err(Failure::Failed("the server took too long to answer".into())),
        Ok(Err(e)) if not_found(&e) => return Err(Failure::Missing),
        Ok(Err(e)) => return Err(Failure::Failed(e.to_string())),
        Ok(Ok(out)) => out,
    };
    if !out.status.success() {
        // ffprobe's own message, last line only; it names no credentials of ours.
        let err = String::from_utf8_lossy(&out.stderr);
        let why = err.lines().last().unwrap_or("could not read the stream");
        return Err(Failure::Failed(why.replace(url, "the stream")));
    }
    // No video line at all means an audio-only stream (radio).
    Ok(Probe::parse(&String::from_utf8_lossy(&out.stdout)))
}

/// NVENC, when this machine really has it: listed by ffmpeg is not the same as working.
async fn nvenc_works() -> bool {
    static WORKS: OnceCell<bool> = OnceCell::const_new();
    *WORKS
        .get_or_init(|| async {
            let test = Command::new("ffmpeg")
                .args([
                    "-v",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "nullsrc=s=256x256:d=0.1",
                ])
                .args(["-c:v", "h264_nvenc", "-f", "null", "-"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .status();
            matches!(timeout(Duration::from_secs(20), test).await, Ok(Ok(s)) if s.success())
        })
        .await
}

/// The ffmpeg command line. `video` is `copy` (leave the video alone) or anything else (encode
/// H.264, deinterlaced, at most `max_height` tall so the browser can decode it smoothly).
fn ffmpeg_args(
    url: &str,
    ua: &str,
    video: &str,
    nvenc: bool,
    height: u32,
    max_height: u32,
) -> Vec<String> {
    let mut a: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-user_agent",
        ua,
        "-protocol_whitelist",
        PROTOCOLS,
        "-reconnect",
        "1",
        "-reconnect_streamed",
        "1",
        "-reconnect_delay_max",
        "4",
        "-rw_timeout",
        "15000000",
        "-fflags",
        "+genpts+discardcorrupt",
        "-analyzeduration",
        "3000000",
        "-probesize",
        "5000000",
    ]
    .map(String::from)
    .into();
    if video != "copy" {
        a.extend(["-hwaccel", "auto"].map(String::from));
    }
    a.extend(["-i", url, "-map", "0:v:0?", "-map", "0:a:0?"].map(String::from));
    if video == "copy" {
        a.extend(["-c:v", "copy"].map(String::from));
    } else {
        let out_height = if height == 0 {
            max_height
        } else {
            height.min(max_height)
        };
        let (rate, buf) = match out_height {
            1800.. => (20, 40),
            1000.. => (10, 20),
            _ => (5, 10),
        };
        let filter = format!(
            "yadif=mode=send_field:deint=interlaced,scale=-2:'min({max_height},ih)',format=yuv420p"
        );
        a.extend(["-vf", &filter].map(String::from));
        if nvenc {
            a.extend(
                [
                    "-c:v",
                    "h264_nvenc",
                    "-preset",
                    "p4",
                    "-profile:v",
                    "high",
                    "-rc",
                    "vbr",
                ]
                .map(String::from),
            );
        } else {
            a.extend(
                [
                    "-c:v",
                    "libx264",
                    "-preset",
                    "veryfast",
                    "-profile:v",
                    "high",
                    "-crf",
                    "22",
                    "-sc_threshold",
                    "0",
                ]
                .map(String::from),
            );
        }
        a.extend(
            [
                "-b:v",
                &format!("{rate}M"),
                "-maxrate",
                &format!("{rate}M"),
                "-bufsize",
                &format!("{buf}M"),
            ]
            .map(String::from),
        );
        a.extend(["-g", "50", "-bf", "0"].map(String::from));
    }
    a.extend(
        [
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ac",
            "2",
            "-af",
            "aresample=async=1:first_pts=0",
        ]
        .map(String::from),
    );
    a.extend(
        [
            "-f",
            "mp4",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            "-frag_duration",
            "1000000",
            "pipe:1",
        ]
        .map(String::from),
    );
    a
}

fn agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .unwrap_or(FALLBACK_UA)
        .to_owned()
}

/// Checks the request is from the app and is for an address the proxy may use.
fn vet(s: &AppState, headers: &HeaderMap, raw: &str) -> Result<Url, (StatusCode, &'static str)> {
    if !from_app(headers) {
        return Err((StatusCode::FORBIDDEN, "only the app may use the proxy"));
    }
    match Url::parse(raw) {
        Ok(url) if url_ok(&s.approved, &url) => Ok(url),
        Ok(_) => Err((StatusCode::FORBIDDEN, "address not allowed")),
        Err(_) => Err((StatusCode::BAD_REQUEST, "bad url")),
    }
}

fn failure(f: Failure) -> Response {
    match f {
        Failure::Missing => refusal(StatusCode::NOT_IMPLEMENTED, MISSING),
        Failure::Failed(why) => refusal(
            StatusCode::BAD_GATEWAY,
            &format!("could not read the stream: {why}"),
        ),
    }
}

/// `GET /compat/check?url=`: can this be converted, and does its video need re-encoding? 204 with
/// `x-riptv-video: copy|transcode` if so, otherwise an explanation in `x-riptv-error`.
pub async fn check(
    State(s): State<AppState>,
    Query(q): Query<CompatQuery>,
    headers: HeaderMap,
) -> Response {
    let url = match vet(&s, &headers, &q.url) {
        Ok(url) => url,
        Err((status, why)) => return refusal(status, why),
    };
    let probed = match probe(url.as_str(), &agent(&headers)).await {
        Ok(p) => p,
        Err(f) => return failure(f),
    };
    let mode = if probed.as_ref().is_none_or(Probe::can_copy) {
        "copy"
    } else {
        "transcode"
    };
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(VIDEO_HEADER, HeaderValue::from_static(mode));
    res
}

/// The ffmpeg child, killed when the browser goes away (the response body is dropped).
struct Piped {
    out: ReaderStream<ChildStdout>,
    _child: Child,
}

impl Stream for Piped {
    type Item = io::Result<Bytes>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.out).poll_next(cx)
    }
}

/// `GET /compat?url=&video=`: the converted stream, as fragmented MP4 for a `<video>` to play.
pub async fn stream(
    State(s): State<AppState>,
    Query(q): Query<CompatQuery>,
    headers: HeaderMap,
) -> Response {
    let url = match vet(&s, &headers, &q.url) {
        Ok(url) => url,
        Err((status, why)) => return refusal(status, why),
    };
    let ua = agent(&headers);
    let (mode, height) = match q.video.as_deref() {
        Some("copy") => ("copy", 0),
        Some("transcode") => ("transcode", probe_height(&url, &ua).await),
        _ => match probe(url.as_str(), &ua).await {
            Ok(Some(p)) if !p.can_copy() => ("transcode", p.height),
            Ok(_) => ("copy", 0),
            Err(f) => return failure(f),
        },
    };
    let max_height = std::env::var("RIPTV_MAX_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1080);
    let nvenc = mode != "copy" && nvenc_works().await;
    let args = ffmpeg_args(url.as_str(), &ua, mode, nvenc, height, max_height);

    let mut child = match Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) if not_found(&e) => return failure(Failure::Missing),
        Err(e) => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                &format!("could not start ffmpeg: {e}"),
            );
        }
    };
    let Some(stdout) = child.stdout.take() else {
        return refusal(StatusCode::BAD_GATEWAY, "could not read ffmpeg's output");
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, "sandbox")
        .body(Body::from_stream(Piped {
            out: ReaderStream::with_capacity(stdout, 64 * 1024),
            _child: child,
        }))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

async fn probe_height(url: &Url, ua: &str) -> u32 {
    match probe(url.as_str(), ua).await {
        Ok(Some(p)) => p.height,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_progressive_h264_is_copied() {
        let p = |codec: &str, pix: &str, field: &str| {
            Probe::parse(&format!(
                "codec_name={codec}\npix_fmt={pix}\nfield_order={field}\nheight=1080\n"
            ))
            .unwrap()
        };
        assert!(p("h264", "yuv420p", "progressive").can_copy());
        assert!(p("h264", "yuv420p", "unknown").can_copy());
        assert!(!p("hevc", "yuv420p", "progressive").can_copy(), "HEVC");
        assert!(
            !p("h264", "yuv420p10le", "progressive").can_copy(),
            "10-bit"
        );
        assert!(!p("h264", "yuv420p", "tt").can_copy(), "interlaced");
        assert!(!p("mpeg2video", "yuv420p", "tt").can_copy());
        // Whatever order ffprobe prints them in, and anything it leaves out counts against copying.
        let shuffled = Probe::parse(
            "height=2160\ncodec_name=hevc\nfield_order=progressive\npix_fmt=yuv420p10le",
        )
        .unwrap();
        assert_eq!((shuffled.codec.as_str(), shuffled.height), ("hevc", 2160));
        assert!(!Probe::parse("codec_name=h264\n").unwrap().can_copy());
        assert_eq!(Probe::parse(""), None, "no video stream: radio");
    }

    fn has(args: &[String], pair: [&str; 2]) -> bool {
        args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1])
    }

    #[test]
    fn the_command_line_copies_or_encodes_as_asked() {
        let copy = ffmpeg_args("http://h/x.ts", "UA", "copy", false, 0, 1080);
        assert!(
            has(&copy, ["-c:v", "copy"]) && !copy.iter().any(|a| a == "-hwaccel" || a == "-vf")
        );
        assert!(has(&copy, ["-c:a", "aac"]) && has(&copy, ["-i", "http://h/x.ts"]));
        assert!(
            has(&copy, ["-protocol_whitelist", PROTOCOLS]),
            "no local files"
        );

        let gpu = ffmpeg_args("http://h/x.ts", "UA", "transcode", true, 2160, 1080);
        assert!(has(&gpu, ["-c:v", "h264_nvenc"]) && has(&gpu, ["-hwaccel", "auto"]));
        assert!(
            has(&gpu, ["-b:v", "10M"]),
            "bitrate follows the capped height, not the source"
        );
        assert!(
            gpu.iter()
                .any(|a| a.contains("min(1080,ih)") && a.contains("yadif"))
        );

        let cpu = ffmpeg_args("http://h/x.ts", "UA", "transcode", false, 2160, 2160);
        assert!(has(&cpu, ["-c:v", "libx264"]) && has(&cpu, ["-b:v", "20M"]));
        assert_eq!(cpu.last().map(String::as_str), Some("pipe:1"));
    }
}
