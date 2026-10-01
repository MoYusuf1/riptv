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
    time::{Duration, Instant},
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
    io::AsyncReadExt,
    process::{Child, ChildStderr, ChildStdout, Command},
    sync::OnceCell,
    time::timeout,
};
use tokio_util::io::ReaderStream;

use crate::{AppState, FALLBACK_UA, diagnostics, from_app, refusal, url_ok};

pub(crate) const MISSING: &str =
    "ffmpeg isn't installed, and this stream needs it (Arch: sudo pacman -S ffmpeg)";
const VIDEO_HEADER: HeaderName = HeaderName::from_static("x-riptv-video");
const HAS_VIDEO_HEADER: HeaderName = HeaderName::from_static("x-riptv-has-video");
const DURATION_HEADER: HeaderName = HeaderName::from_static("x-riptv-duration");

/// Only plain network protocols: a playlist from a provider must not be able to make ffmpeg read
/// local files.
const PROTOCOLS: &str = "http,https,tcp,tls,crypto";

#[derive(Deserialize)]
pub struct CompatQuery {
    url: String,
    /// `copy` or `transcode`, as reported by the check; looked up again if absent.
    video: Option<String>,
    /// Whole seconds into a movie or episode to begin at; the page's seek bar restarts the
    /// conversion here, because a converted stream has no index a `<video>` could seek in.
    start: Option<u32>,
}

/// What the first video stream is, from ffprobe.
#[derive(Debug, PartialEq)]
struct Probe {
    codec: String,
    pix_fmt: String,
    field_order: String,
    height: u32,
    /// Seconds, for a movie or episode; live streams have none.
    duration: Option<f64>,
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
            duration: get("duration")
                .parse::<f64>()
                .ok()
                .filter(|d| d.is_finite() && *d > 0.0),
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
        .arg("stream=codec_name,pix_fmt,field_order,height:format=duration")
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
pub(crate) async fn nvenc_works() -> bool {
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
    start: u32,
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
    if start > 0 {
        // Before `-i`: jump there without reading everything before it. Timestamps then begin at
        // zero, so the page adds `start` back when it shows the position.
        a.extend(["-ss".to_string(), start.to_string()]);
    }
    a.extend(["-i", url].map(String::from));
    a.extend(encode_args(video, nvenc, height, max_height, "aac"));
    a
}

/// Everything after the input: which tracks, how the video and sound are encoded (`video` is
/// `copy` or `transcode`; `audio` is `copy` or `aac`), and fragmented MP4 out on stdout.
pub(crate) fn encode_args(
    video: &str,
    nvenc: bool,
    height: u32,
    max_height: u32,
    audio: &str,
) -> Vec<String> {
    let mut a: Vec<String> = ["-map", "0:v:0?", "-map", "0:a:0?"]
        .map(String::from)
        .into();
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
    if audio == "copy" {
        // Transport streams frame AAC as ADTS; MP4 wants it bare.
        a.extend(["-c:a", "copy", "-bsf:a", "aac_adtstoasc"].map(String::from));
    } else {
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
    }
    a.extend(
        [
            "-f",
            "mp4",
            "-movflags",
            "frag_keyframe+empty_moov+default_base_moof",
            // Half-second fragments: the browser can start on the first one.
            "-frag_duration",
            "500000",
            "pipe:1",
        ]
        .map(String::from),
    );
    a
}

pub(crate) fn agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .unwrap_or(FALLBACK_UA)
        .to_owned()
}

/// Checks the request is from the app and is for an address the proxy may use.
pub(crate) fn vet(
    s: &AppState,
    headers: &HeaderMap,
    raw: &str,
) -> Result<Url, (StatusCode, &'static str)> {
    if !from_app(headers) {
        return Err((StatusCode::FORBIDDEN, "only the app may use the proxy"));
    }
    match Url::parse(raw) {
        Ok(url) if url_ok(&s.approved, &url) => Ok(url),
        Ok(_) => Err((StatusCode::FORBIDDEN, "address not allowed")),
        Err(_) => Err((StatusCode::BAD_REQUEST, "bad url")),
    }
}

fn failure(s: &AppState, url: &Url, f: Failure) -> Response {
    if let Failure::Failed(why) = &f {
        diagnostics::note(
            s,
            &format!(
                "{}ffprobe could not read the {}: {}",
                diagnostics::tag(s, url),
                diagnostics::kind_of(url),
                diagnostics::redact(why, Some(url), 200)
            ),
        );
    }
    match f {
        Failure::Missing => refusal(StatusCode::NOT_IMPLEMENTED, MISSING),
        Failure::Failed(why) => refusal(
            StatusCode::BAD_GATEWAY,
            &format!("could not read the stream: {why}"),
        ),
    }
}

/// `GET /compat/check?url=`: can this be converted, and does its video need re-encoding? 204 with
/// `x-riptv-video: copy|transcode` and `x-riptv-has-video: 0|1` if so, otherwise an explanation
/// in `x-riptv-error`. Audio-only sources remain valid for normal conversion.
pub async fn check(
    State(s): State<AppState>,
    Query(q): Query<CompatQuery>,
    headers: HeaderMap,
) -> Response {
    let url = match vet(&s, &headers, &q.url) {
        Ok(url) => url,
        Err((status, why)) => return refusal(status, why),
    };
    let ua = agent(&headers);
    let input = one_variant(&s, &url, &ua).await;
    let probed = match probe(input.as_str(), &ua).await {
        Ok(p) => p,
        Err(f) => return failure(&s, &url, f),
    };
    let mode = if probed.as_ref().is_none_or(Probe::can_copy) {
        "copy"
    } else {
        "transcode"
    };
    let mut res = StatusCode::NO_CONTENT.into_response();
    res.headers_mut()
        .insert(VIDEO_HEADER, HeaderValue::from_static(mode));
    res.headers_mut().insert(
        HAS_VIDEO_HEADER,
        HeaderValue::from_static(if probed.is_some() { "1" } else { "0" }),
    );
    // A movie or episode's length, so the page can offer a seek bar.
    if let Some(secs) = probed.as_ref().and_then(|p| p.duration)
        && let Ok(v) = HeaderValue::from_str(&(secs as u64).to_string())
    {
        res.headers_mut().insert(DURATION_HEADER, v);
    }
    res
}

/// The ffmpeg child, killed when the browser goes away (the response body is dropped).
pub(crate) struct Piped {
    pub(crate) out: ReaderStream<ChildStdout>,
    pub(crate) _child: Child,
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
    let input = one_variant(&s, &url, &ua).await;
    let (mode, height) = match q.video.as_deref() {
        Some("copy") => ("copy", 0),
        // The scaler caps the height itself; probing just to learn it cost ~2 s.
        Some("transcode") => ("transcode", 0),
        _ => match probe(input.as_str(), &ua).await {
            Ok(Some(p)) if !p.can_copy() => ("transcode", p.height),
            Ok(_) => ("copy", 0),
            Err(f) => return failure(&s, &url, f),
        },
    };
    let max_height = max_height();
    let nvenc = mode != "copy" && nvenc_works().await;
    let mut args = ffmpeg_args(
        input.as_str(),
        &ua,
        mode,
        nvenc,
        height,
        max_height,
        q.start.unwrap_or(0),
    );
    // With `--logs`, ffmpeg reports its speed every few seconds (on stderr, even at
    // `-loglevel error`), which tells a conversion that can't keep up from a slow source.
    let logging = s.logs.is_some();
    if logging {
        args.splice(0..0, ["-stats", "-stats_period", "5"].map(String::from));
        diagnostics::note(
            &s,
            &format!(
                "{}ffmpeg start {} video={mode}{} height={height} start={}",
                diagnostics::tag(&s, &url),
                diagnostics::kind_of(&url),
                if mode == "copy" {
                    ""
                } else if nvenc {
                    " encoder=nvenc"
                } else {
                    " encoder=libx264"
                },
                q.start.unwrap_or(0)
            ),
        );
    }

    let mut child = match Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if logging {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) if not_found(&e) => return failure(&s, &url, Failure::Missing),
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
    if let Some(stderr) = child.stderr.take() {
        let tag = diagnostics::tag(&s, &url);
        tokio::spawn(watch_ffmpeg(s.clone(), url.clone(), tag, stderr));
    }
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

/// The media playlist to convert, when `url` is an HLS master playlist: ffmpeg and ffprobe
/// otherwise open every variant to probe it, which costs seconds at start (measured: 11.6 s to the
/// first output for a 5-variant master, 3 s for one variant). The highest-bandwidth variant no
/// taller than `RIPTV_MAX_HEIGHT` is chosen. Anything else, or any failure, is `url` unchanged.
async fn one_variant(s: &AppState, url: &Url, ua: &str) -> Url {
    let fetch = async {
        let mut response = s
            .http
            .get(url.clone())
            .header(header::USER_AGENT, ua)
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let base = response.url().clone();
        let mut body = Vec::new();
        while body.len() < 256 * 1024 {
            match response.chunk().await.ok()? {
                Some(chunk) => body.extend_from_slice(&chunk),
                None => break,
            }
            // Not a playlist (a live `.ts` stream, say): stop reading at once.
            if body.len() >= 7 && !body.starts_with(b"#EXTM3U") {
                return None;
            }
        }
        let text = String::from_utf8_lossy(&body);
        let (path, height, count) = pick_variant(&text, max_height())?;
        let chosen = base.join(path).ok().filter(|v| url_ok(&s.approved, v))?;
        Some((chosen, height, count))
    };
    match timeout(Duration::from_secs(8), fetch).await {
        Ok(Some((chosen, height, count))) => {
            diagnostics::note(
                s,
                &format!(
                    "{}hls master playlist: converting the {}p variant of {count}",
                    diagnostics::tag(s, url),
                    height.map_or("?".into(), |h| h.to_string())
                ),
            );
            chosen
        }
        _ => url.clone(),
    }
}

pub(crate) fn max_height() -> u32 {
    std::env::var("RIPTV_MAX_HEIGHT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1080)
}

/// From a master playlist's `#EXT-X-STREAM-INF` entries: the highest bandwidth no taller than
/// `max_height` (or the shortest, if all are taller). Returns its URI, height, and how many there
/// were; `None` if this isn't a master playlist.
pub(crate) fn pick_variant(playlist: &str, max_height: u32) -> Option<(&str, Option<u32>, usize)> {
    let mut variants = vec![];
    let mut lines = playlist.lines().map(str::trim);
    while let Some(line) = lines.next() {
        let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") else {
            continue;
        };
        let attr = |name: &str| {
            attrs
                .split(',')
                .find_map(|a| a.trim().strip_prefix(name)?.strip_prefix('='))
        };
        let bandwidth: u64 = attr("BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
        let height: Option<u32> = attr("RESOLUTION")
            .and_then(|r| r.split_once('x'))
            .and_then(|(_, h)| h.parse().ok());
        if let Some(uri) = lines.find(|l| !l.is_empty() && !l.starts_with('#')) {
            variants.push((uri, height, bandwidth));
        }
    }
    let count = variants.len();
    let fits = |v: &&(&str, Option<u32>, u64)| v.1.is_none_or(|h| h <= max_height);
    let best = variants
        .iter()
        .filter(fits)
        .max_by_key(|v| v.2)
        .or_else(|| variants.iter().min_by_key(|v| v.1))?;
    Some((best.0, best.1, count))
}

/// What ffmpeg says while it converts, into the diagnostics log: its errors (redacted), when its
/// first output came, and any stretch where it ran slower than real time (the viewer buffers).
pub(crate) async fn watch_ffmpeg(s: AppState, url: Url, tag: String, mut stderr: ChildStderr) {
    const MAX_ERRORS: usize = 40;
    let started = Instant::now();
    let (mut buf, mut pending) = ([0_u8; 4096], Vec::<u8>::new());
    let (mut errors, mut first, mut slow, mut last) = (0, true, false, String::new());
    loop {
        let n = match stderr.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        pending.extend_from_slice(&buf[..n]);
        while let Some(end) = pending.iter().position(|&b| b == b'\r' || b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line).trim().to_owned();
            if line.is_empty() {
                continue;
            }
            if let Some(progress) = Progress::parse(&line) {
                let summary = progress.to_string();
                if first {
                    first = false;
                    diagnostics::note(
                        &s,
                        &format!(
                            "{tag}ffmpeg first progress after {}ms: {summary}",
                            started.elapsed().as_millis()
                        ),
                    );
                } else if progress.speed < 0.95 && progress.time > 5.0 {
                    slow = true;
                    diagnostics::note(&s, &format!("{tag}ffmpeg slower than real time: {summary}"));
                } else if slow && progress.speed >= 0.95 {
                    slow = false;
                    diagnostics::note(&s, &format!("{tag}ffmpeg back to real time: {summary}"));
                }
                last = summary;
            } else if benign(&line) {
                continue;
            } else if errors < MAX_ERRORS {
                errors += 1;
                diagnostics::note(
                    &s,
                    &format!(
                        "{tag}ffmpeg says: {}",
                        diagnostics::redact(&line, Some(&url), 200)
                    ),
                );
            }
        }
    }
    diagnostics::note(
        &s,
        &format!(
            "{tag}ffmpeg ended after {}s (last: {})",
            started.elapsed().as_secs(),
            if last.is_empty() { "no output" } else { &last }
        ),
    );
}

/// What every live join prints until the first keyframe (the decoder starts mid-stream): not news.
fn benign(line: &str) -> bool {
    [
        "non-existing PPS",
        "no frame!",
        "decode_slice_header error",
        "Last message repeated",
        "co located POCs unavailable",
    ]
    .iter()
    .any(|noise| line.contains(noise))
}

/// One of ffmpeg's `-stats` lines: `frame= 75 fps= 30 … time=00:00:03.00 … speed=1.22x`.
struct Progress {
    fps: Option<f64>,
    time: f64,
    speed: f64,
}

impl Progress {
    fn parse(line: &str) -> Option<Self> {
        if !(line.starts_with("frame=") || line.starts_with("size=")) {
            return None;
        }
        // Values may be padded after `=`: join each key to its value first.
        let mut tidy = line.to_owned();
        while tidy.contains("= ") {
            tidy = tidy.replace("= ", "=");
        }
        let field = |key: &str| {
            tidy.split_whitespace()
                .find_map(|w| w.strip_prefix(key))
                .map(str::to_owned)
        };
        let time = field("time=").and_then(|t| {
            let mut secs = 0.0;
            for part in t.split(':') {
                secs = secs * 60.0 + part.parse::<f64>().ok()?;
            }
            Some(secs)
        })?;
        let speed = field("speed=")?.trim_end_matches('x').parse().ok()?;
        Some(Self {
            fps: field("fps=").and_then(|f| f.parse().ok()),
            time,
            speed,
        })
    }
}

impl std::fmt::Display for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "speed={:.2}x media_time={:.0}s", self.speed, self.time)?;
        if let Some(fps) = self.fps {
            write!(f, " fps={fps:.0}")?;
        }
        Ok(())
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
        // A movie has a length; a live stream says N/A.
        let long = Probe::parse("codec_name=h264\nduration=7200.5\n").unwrap();
        assert_eq!(long.duration, Some(7200.5));
        assert_eq!(
            Probe::parse("codec_name=h264\nduration=N/A\n")
                .unwrap()
                .duration,
            None
        );
    }

    #[test]
    fn the_best_variant_that_fits_is_converted() {
        let master = "#EXTM3U\n\
            #EXT-X-STREAM-INF:BANDWIDTH=2149280,CODECS=\"mp4a.40.2,avc1.64001f\",RESOLUTION=1280x720\nhd.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=246440,RESOLUTION=320x184\nld.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=6221600,RESOLUTION=1920x1080\nfhd.m3u8\n\
            #EXT-X-STREAM-INF:BANDWIDTH=16000000,RESOLUTION=3840x2160\nuhd.m3u8\n";
        assert_eq!(
            pick_variant(master, 1080),
            Some(("fhd.m3u8", Some(1080), 4))
        );
        assert_eq!(pick_variant(master, 720), Some(("hd.m3u8", Some(720), 4)));
        assert_eq!(pick_variant(master, 100), Some(("ld.m3u8", Some(184), 4)));
        // No resolutions: the highest bandwidth.
        assert_eq!(
            pick_variant(
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\na.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=9\nb.m3u8\n",
                1080
            ),
            Some(("b.m3u8", None, 2))
        );
        // A media playlist is not a master.
        assert_eq!(pick_variant("#EXTM3U\n#EXTINF:4,\na.ts\n", 1080), None);
    }

    #[test]
    fn ffmpeg_progress_lines_are_read() {
        let p = Progress::parse("frame=   55 fps= 27 q=16.0 size=       1KiB time=00:00:02.20 bitrate=   2.9kbits/s speed= 1.1x elapsed=0:00:02.00").unwrap();
        assert_eq!((p.fps, p.time, p.speed), (Some(27.0), 2.2, 1.1));
        // Audio only: no frame count.
        let p =
            Progress::parse("size=     256KiB time=01:00:05.50 bitrate= 128.0kbits/s speed=0.83x")
                .unwrap();
        assert_eq!((p.fps, p.time, p.speed), (None, 3605.5, 0.83));
        assert!(Progress::parse("[hls @ 0x55] Opening 'x' for reading").is_none());
        assert!(
            Progress::parse("frame=    0 fps=0.0 q=0.0 size=0KiB time=N/A bitrate=N/A speed=N/A")
                .is_none()
        );
    }

    fn has(args: &[String], pair: [&str; 2]) -> bool {
        args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1])
    }

    #[test]
    fn the_command_line_copies_or_encodes_as_asked() {
        let copy = ffmpeg_args("http://h/x.ts", "UA", "copy", false, 0, 1080, 0);
        assert!(
            has(&copy, ["-c:v", "copy"]) && !copy.iter().any(|a| a == "-hwaccel" || a == "-vf")
        );
        assert!(has(&copy, ["-c:a", "aac"]) && has(&copy, ["-i", "http://h/x.ts"]));
        assert!(
            has(&copy, ["-protocol_whitelist", PROTOCOLS]),
            "no local files"
        );

        // A seek goes before the input (so nothing earlier is read); no seek, no flag.
        assert!(!copy.iter().any(|a| a == "-ss"));
        let seek = ffmpeg_args("http://h/x.mp4", "UA", "copy", false, 0, 1080, 754);
        let at = |flag: &str| seek.iter().position(|a| a == flag).unwrap();
        assert!(at("-ss") < at("-i") && seek[at("-ss") + 1] == "754");

        let gpu = ffmpeg_args("http://h/x.ts", "UA", "transcode", true, 2160, 1080, 0);
        assert!(has(&gpu, ["-c:v", "h264_nvenc"]) && has(&gpu, ["-hwaccel", "auto"]));
        assert!(
            has(&gpu, ["-b:v", "10M"]),
            "bitrate follows the capped height, not the source"
        );
        assert!(
            gpu.iter()
                .any(|a| a.contains("min(1080,ih)") && a.contains("yadif"))
        );

        let cpu = ffmpeg_args("http://h/x.ts", "UA", "transcode", false, 2160, 2160, 0);
        assert!(has(&cpu, ["-c:v", "libx264"]) && has(&cpu, ["-b:v", "20M"]));
        assert_eq!(cpu.last().map(String::as_str), Some("pipe:1"));
    }
}
