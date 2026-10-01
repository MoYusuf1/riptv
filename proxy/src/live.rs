//! `GET /live?url=&can=&video=`: a live channel as one fragmented-MP4 stream, started as fast as
//! the provider allows. This is the standard player's whole live path.
//!
//! The proxy reads the channel itself: an HLS playlist is followed here (starting at the newest
//! segment but one, which already exists, so bytes flow at once) and a continuous transport stream
//! is read as it comes. The codecs are read from those same first bytes ([`xtream::sniff`]): no
//! second request, which matters because many providers allow one connection per channel. ffmpeg,
//! fed through a pipe, then copies what the browser decodes and converts only what it can't: the
//! sound (AC-3, E-AC-3, MP2, LATM) or the video (HEVC it can't decode, MPEG-2, interlaced).
//!
//! The browser gets a single response: it never runs an HLS player, never waits for a whole
//! segment, and starts on the first half-second fragment.

use std::{process::Stdio, time::Duration};

use axum::{
    body::{Body, Bytes},
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use reqwest::Url;
use serde::Deserialize;
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    sync::mpsc,
    time::{Instant, sleep, timeout},
};
use tokio_util::io::ReaderStream;
use xtream::{Sniff, sniff::Plan};

use crate::{
    AppState,
    compat::{
        MISSING, Piped, agent, encode_args, max_height, nvenc_works, pick_variant, vet,
        watch_ffmpeg,
    },
    diagnostics, refusal,
};

#[derive(Deserialize)]
pub struct LiveQuery {
    url: String,
    /// Codecs this browser decodes beyond the basics (H.264, AAC): `hevc`, `ac3`, `eac3`.
    #[serde(default)]
    can: String,
    /// `transcode` to re-encode the video whatever it is (the browser rejected it once).
    video: Option<String>,
}

/// How much of the stream is read, at most, before deciding: the program map comes in the first
/// packets, but the first keyframe (where the picture can begin) can be a few seconds in when a
/// provider cuts segments mid-GOP.
const SNIFF_MAX: usize = 4 * 1024 * 1024;
/// Chunks in flight between the provider and ffmpeg.
const QUEUE: usize = 32;
/// A playlist this large is not a playlist.
const PLAYLIST_MAX: usize = 1024 * 1024;

pub async fn stream(
    State(s): State<AppState>,
    Query(q): Query<LiveQuery>,
    headers: HeaderMap,
) -> Response {
    let url = match vet(&s, &headers, &q.url) {
        Ok(url) => url,
        Err((status, why)) => return refusal(status, why),
    };
    let ua = agent(&headers);
    let tag = diagnostics::tag(&s, &url);
    let started = Instant::now();

    // The provider's bytes, in order, from a task that follows the channel.
    let (tx, mut rx) = mpsc::channel::<Bytes>(QUEUE);
    // Noted before asking, so a provider too slow to answer at all still shows in the log.
    diagnostics::note(&s, &format!("{tag}live asking the provider"));
    let mut steps = vec![];
    let source = match Source::open(&s, &url, &ua, &mut steps).await {
        Ok(source) => source,
        Err(why) => {
            diagnostics::note(&s, &format!("{tag}live could not start: {why}"));
            return refusal(
                StatusCode::BAD_GATEWAY,
                &format!("could not read the stream: {why}"),
            );
        }
    };
    let kind = source.kind();
    // Both tasks die with the response (or with this handler, if the viewer leaves during the
    // start): nothing keeps reading a channel nobody watches. That matters twice over: a
    // provider counts every open connection against the account's limit.
    let follower = AbortOnDrop(
        tokio::spawn(source.run(s.clone(), ua.clone(), tag.clone(), tx)).abort_handle(),
    );

    // Enough of the start to read the codecs from.
    let mut head = Vec::with_capacity(128 * 1024);
    let first_byte = match timeout(Duration::from_secs(15), rx.recv()).await {
        Ok(Some(chunk)) => {
            head.extend_from_slice(&chunk);
            started.elapsed()
        }
        Ok(None) => return refusal(StatusCode::BAD_GATEWAY, "the stream ended before it began"),
        Err(_) => {
            return refusal(
                StatusCode::GATEWAY_TIMEOUT,
                "the provider sent nothing for 15 s",
            );
        }
    };
    let sniffed = loop {
        let found = Sniff::of(&head);
        // Known when it isn't a transport stream, or its map is read and (for a picture) where
        // its first keyframe is.
        let settled = found.container != "mpeg_ts"
            || (found.video.is_some() || found.audio.is_some())
                && (found.video.is_none() || found.keyframe_at.is_some());
        if settled || head.len() >= SNIFF_MAX {
            break found;
        }
        match timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(chunk)) => head.extend_from_slice(&chunk),
            _ => break found,
        }
    };

    let can = |mime: &str| {
        (mime.contains("hvc1") && q.can.contains("hevc"))
            || (mime.contains("ac-3") && q.can.contains("ac3"))
            || (mime.contains("ec-3") && q.can.contains("eac3"))
    };
    let plan = xtream::sniff::plan(&sniffed, can);
    // Interlaced pictures are deinterlaced: browsers either reject them or show combing.
    let transcode = plan == Plan::ConvertVideo
        || sniffed.interlaced == Some(true)
        || q.video.as_deref() == Some("transcode");
    let video = if transcode { "transcode" } else { "copy" };
    // Sound is always re-encoded as plain AAC: it costs little, and copied AAC breaks browsers in
    // ways a sniff can't see (HE-AAC signalling, odd configurations, a first frame cut short).
    let audio = "aac";
    // A few bytes of text where video should be: the provider's own error message ("Cannot read
    // source", say). Say what it said, at once.
    if sniffed.container == "unknown"
        && head.len() < 1024
        && head
            .iter()
            .all(|b| b.is_ascii_graphic() || b.is_ascii_whitespace())
    {
        let said = diagnostics::redact(&String::from_utf8_lossy(&head), Some(&url), 120);
        diagnostics::note(
            &s,
            &format!("{tag}live the provider sent text, not video: {said:?}"),
        );
        return refusal(
            StatusCode::BAD_GATEWAY,
            &format!("the provider says this channel is unavailable: {said}"),
        );
    }
    if sniffed.container == "unknown" {
        // What it starts with, as hex: a signature, never text that could carry an address.
        let signature: String = head.iter().take(12).map(|b| format!("{b:02x}")).collect();
        diagnostics::note(
            &s,
            &format!(
                "{tag}live unrecognised stream: {} bytes, starting {signature}",
                head.len()
            ),
        );
    }
    diagnostics::note(
        &s,
        &format!(
            "{tag}live {kind}: {} → first bytes after {}ms, read in {}ms: {} video={} audio={} interlaced={} height={} keyframe_at={} → video {video}, audio {audio}",
            steps.join(", "),
            first_byte.as_millis(),
            started.elapsed().as_millis(),
            sniffed.container,
            sniffed.video.unwrap_or("-"),
            sniffed.audio.unwrap_or("-"),
            sniffed.interlaced.map_or("-".into(), |i| i.to_string()),
            sniffed.height.map_or("-".into(), |h| h.to_string()),
            sniffed.keyframe_at.map_or("-".into(), |k| k.to_string()),
        ),
    );

    // Start ffmpeg at the first keyframe, with the tables it needs in front: bytes before it can't
    // be shown, and without its parameter sets ffmpeg can't even size the picture.
    let head = match (sniffed.keyframe_at, sniffed.pat_at, sniffed.pmt_at) {
        (Some(key), Some(pat), Some(pmt)) if key > 0 => {
            let mut trimmed = Vec::with_capacity(head.len() - key + 376);
            trimmed.extend_from_slice(&head[pat..pat + 188]);
            trimmed.extend_from_slice(&head[pmt..pmt + 188]);
            trimmed.extend_from_slice(&head[key..]);
            trimmed
        }
        _ => head,
    };
    let nvenc = transcode && nvenc_works().await;
    let mut args: Vec<String> = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-fflags",
        "+genpts+discardcorrupt",
        // Limits, not waits: ffmpeg stops probing once every track is known, which with the bytes
        // already here is at once; a 4K stream needs megabytes before its first sound.
        "-probesize",
        "5000000",
        "-analyzeduration",
        "2000000",
    ]
    .map(String::from)
    .into();
    if s.logs.is_some() {
        args.extend(["-stats", "-stats_period", "5"].map(String::from));
    }
    // Hardware decoding only where it pays: 4K HEVC. Setting it up costs more than decoding
    // H.264 in software.
    if transcode && sniffed.video == Some("hevc") {
        args.extend(["-hwaccel", "auto"].map(String::from));
    }
    // A transport stream is said so (no guessing); anything else ffmpeg recognises itself.
    if sniffed.container == "mpeg_ts" {
        args.extend(["-f", "mpegts"].map(String::from));
    }
    args.extend(["-i", "pipe:0"].map(String::from));
    args.extend(encode_args(
        video,
        nvenc,
        sniffed.height.unwrap_or(0),
        max_height(),
        audio,
    ));
    let mut child = match Command::new("ffmpeg")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if s.logs.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return refusal(StatusCode::NOT_IMPLEMENTED, MISSING);
        }
        Err(e) => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                &format!("could not start ffmpeg: {e}"),
            );
        }
    };
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return refusal(StatusCode::BAD_GATEWAY, "could not talk to ffmpeg");
    };
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(watch_ffmpeg(s.clone(), url.clone(), tag.clone(), stderr));
    }
    // Feed ffmpeg: what was read already, then the rest as it arrives. Ends when the viewer
    // leaves (ffmpeg dies with the response, so the write fails) or the channel does.
    let feeder = AbortOnDrop(
        tokio::spawn(async move {
            if stdin.write_all(&head).await.is_err() {
                return;
            }
            while let Some(chunk) = rx.recv().await {
                if stdin.write_all(&chunk).await.is_err() {
                    return;
                }
            }
        })
        .abort_handle(),
    );

    let mut out = ReaderStream::with_capacity(stdout, 64 * 1024);
    // Wait for ffmpeg's first output so its time to start is known (and a dead start is an error).
    let first = match timeout(Duration::from_secs(20), futures_next(&mut out)).await {
        Ok(Some(Ok(bytes))) => bytes,
        _ => {
            return refusal(
                StatusCode::BAD_GATEWAY,
                "ffmpeg produced nothing from this stream",
            );
        }
    };
    diagnostics::note(
        &s,
        &format!(
            "{tag}live first output after {}ms",
            started.elapsed().as_millis()
        ),
    );
    let rest = Piped { out, _child: child };
    let body = futures_prepend(first, rest, [follower, feeder]);
    Response::builder()
        .header(header::CONTENT_TYPE, "video/mp4")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::CONTENT_SECURITY_POLICY, "sandbox")
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
}

async fn futures_next<S: futures_core::Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    std::future::poll_fn(|cx| std::pin::Pin::new(&mut *stream).poll_next(cx)).await
}

/// `first`, then everything `rest` yields.
fn futures_prepend(
    first: Bytes,
    rest: Piped,
    tasks: [AbortOnDrop; 2],
) -> impl futures_core::Stream<Item = std::io::Result<Bytes>> + Send {
    struct Prepended {
        first: Option<Bytes>,
        rest: Piped,
        _tasks: [AbortOnDrop; 2],
    }
    impl futures_core::Stream for Prepended {
        type Item = std::io::Result<Bytes>;
        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<Self::Item>> {
            if let Some(first) = self.first.take() {
                return std::task::Poll::Ready(Some(Ok(first)));
            }
            std::pin::Pin::new(&mut self.rest).poll_next(cx)
        }
    }
    Prepended {
        first: Some(first),
        rest,
        _tasks: tasks,
    }
}

/// A task that ends when this does.
struct AbortOnDrop(tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Where the channel's bytes come from.
enum Source {
    /// An HLS media playlist, followed from `next` on.
    Hls {
        playlist: Url,
        media: Media,
        next: u64,
    },
    /// A continuous transport stream (`.ts`), already answering.
    Continuous(reqwest::Response, Bytes),
}

impl Source {
    fn kind(&self) -> &'static str {
        match self {
            Self::Hls { .. } => "hls",
            Self::Continuous(..) => "ts",
        }
    }

    /// Connects: a playlist is read (a master playlist's best variant chosen), a stream left open.
    async fn open(
        s: &AppState,
        url: &Url,
        ua: &str,
        steps: &mut Vec<String>,
    ) -> Result<Self, String> {
        let mut current = url.clone();
        for _ in 0..2 {
            let asked = Instant::now();
            let mut response = get(s, &current, ua).await?;
            let redirected = response.url() != &current;
            let base = response.url().clone();
            let first = response
                .chunk()
                .await
                .map_err(|_| "the connection broke".to_string())?
                .unwrap_or_default();
            if !first.starts_with(b"#EXTM3U") {
                steps.push(format!("stream {}ms", asked.elapsed().as_millis()));
                return Ok(Self::Continuous(response, first));
            }
            let text = read_rest(response, first).await?;
            steps.push(format!(
                "playlist {}ms{}",
                asked.elapsed().as_millis(),
                if redirected { " (redirected)" } else { "" }
            ));
            if let Some((variant, ..)) = pick_variant(&text, max_height()) {
                current = base
                    .join(variant)
                    .map_err(|_| "a bad playlist link".to_string())?;
                continue;
            }
            let media = Media::parse(&text, &base);
            let next = media.start();
            return Ok(Self::Hls {
                playlist: base,
                media,
                next,
            });
        }
        Err("the playlist nests too deeply".into())
    }

    /// Sends the channel's bytes into `tx` until it ends or nobody is listening.
    async fn run(self, s: AppState, ua: String, tag: String, tx: mpsc::Sender<Bytes>) {
        match self {
            Self::Continuous(mut response, first) => {
                if tx.send(first).await.is_err() {
                    return;
                }
                while let Ok(Some(chunk)) = response.chunk().await {
                    if tx.send(chunk).await.is_err() {
                        return;
                    }
                }
                diagnostics::note(&s, &format!("{tag}live stream ended by the provider"));
            }
            Self::Hls {
                playlist,
                mut media,
                mut next,
            } => {
                let mut quiet_since = Instant::now();
                let mut refreshed = Instant::now();
                loop {
                    let from = next;
                    for (seq, segment, _) in media.segments.iter().filter(|(seq, ..)| *seq >= from)
                    {
                        let started = Instant::now();
                        let Ok(mut response) = get(&s, segment, &ua).await else {
                            diagnostics::note(&s, &format!("{tag}live segment {seq} failed"));
                            next = seq + 1;
                            continue;
                        };
                        while let Ok(Some(chunk)) = response.chunk().await {
                            if tx.send(chunk).await.is_err() {
                                return; // the viewer left
                            }
                        }
                        let took = started.elapsed().as_secs_f64();
                        if took > media.target {
                            diagnostics::note(
                                &s,
                                &format!(
                                    "{tag}live segment {seq} took {took:.1}s for {:.0}s of video: the provider can't keep up",
                                    media.target
                                ),
                            );
                        }
                        next = seq + 1;
                        quiet_since = Instant::now();
                    }
                    if media.ended {
                        return;
                    }
                    if quiet_since.elapsed().as_secs_f64() > media.target * 6.0 {
                        diagnostics::note(&s, &format!("{tag}live playlist stopped advancing"));
                        return;
                    }
                    // Look again after half a segment (sooner never finds anything new).
                    let wait = Duration::from_secs_f64((media.target / 2.0).max(0.5));
                    if let Some(left) = wait.checked_sub(refreshed.elapsed()) {
                        sleep(left).await;
                    }
                    if tx.is_closed() {
                        return;
                    }
                    refreshed = Instant::now();
                    match get(&s, &playlist, &ua).await {
                        Ok(response) => {
                            if let Ok(text) = read_rest(response, Bytes::new()).await {
                                media = Media::parse(&text, &playlist);
                            }
                        }
                        Err(why) => {
                            diagnostics::note(&s, &format!("{tag}live playlist refresh: {why}"))
                        }
                    }
                }
            }
        }
    }
}

async fn get(s: &AppState, url: &Url, ua: &str) -> Result<reqwest::Response, String> {
    let response = s
        .http
        .get(url.clone())
        .header(header::USER_AGENT, ua)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                "the provider didn't answer".to_string()
            } else {
                "could not connect to the provider".to_string()
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!(
            "Server returned {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        ));
    }
    Ok(response)
}

async fn read_rest(mut response: reqwest::Response, first: Bytes) -> Result<String, String> {
    let mut body = first.to_vec();
    while body.len() < PLAYLIST_MAX {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            Ok(None) => break,
            Err(_) => return Err("the connection broke".into()),
        }
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Seconds of video, already on the server, to start from: all of it arrives at once, so ffmpeg
/// sets up and the browser buffers without waiting on real time (1-second segments otherwise
/// trickle in), with a cushion against the next segment coming late.
const START_SECONDS: f64 = 6.0;

/// A media playlist: its segments with their sequence numbers.
struct Media {
    /// Seconds per segment, at most.
    target: f64,
    sequence: u64,
    /// Sequence number, address, seconds.
    segments: Vec<(u64, Url, f64)>,
    ended: bool,
}

impl Media {
    fn parse(text: &str, base: &Url) -> Self {
        let tag = |name: &str| {
            text.lines()
                .find_map(|l| l.trim().strip_prefix(name))
                .map(str::trim)
        };
        let sequence = tag("#EXT-X-MEDIA-SEQUENCE:")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let mut segments = vec![];
        let mut seconds = 0.0;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some(info) = line.strip_prefix("#EXTINF:") {
                seconds = info
                    .split(',')
                    .next()
                    .and_then(|d| d.trim().parse().ok())
                    .unwrap_or(0.0);
            } else if !line.starts_with('#')
                && let Ok(url) = base.join(line)
            {
                segments.push((sequence + segments.len() as u64, url, seconds));
            }
        }
        Self {
            target: tag("#EXT-X-TARGETDURATION:")
                .and_then(|v| v.parse().ok())
                .unwrap_or(6.0_f64)
                .max(1.0),
            sequence,
            segments,
            ended: text.contains("#EXT-X-ENDLIST"),
        }
    }
}

impl Media {
    /// Where to begin: at least two segments and [`START_SECONDS`] back from the newest.
    fn start(&self) -> u64 {
        let mut seconds = 0.0;
        for (taken, (seq, _, length)) in self.segments.iter().rev().enumerate() {
            seconds += if *length > 0.0 { *length } else { self.target };
            if taken >= 1 && seconds >= START_SECONDS {
                return *seq;
            }
        }
        self.segments.first().map_or(self.sequence, |s| s.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_numbered_from_the_media_sequence() {
        let base = Url::parse("https://h.tv/live/u/p/1.m3u8").unwrap();
        let m = Media::parse(
            "#EXTM3U\n#EXT-X-TARGETDURATION:10\n#EXT-X-MEDIA-SEQUENCE:500\n#EXTINF:10,\na.ts\n#EXTINF:10,\n/abs/b.ts\n",
            &base,
        );
        assert_eq!(m.target, 10.0);
        assert!(!m.ended);
        assert_eq!(
            m.segments[0],
            (500, Url::parse("https://h.tv/live/u/p/a.ts").unwrap(), 10.0)
        );
        assert_eq!(
            m.segments[1],
            (501, Url::parse("https://h.tv/abs/b.ts").unwrap(), 10.0)
        );
        // Two 10 s segments: start at the older of the two.
        assert_eq!(m.start(), 500);
    }

    #[test]
    fn short_segments_start_far_enough_back() {
        let base = Url::parse("https://h.tv/1.m3u8").unwrap();
        let mut text =
            String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:100\n");
        for i in 0..10 {
            text += &format!("#EXTINF:1.0,\n{i}.ts\n");
        }
        // Six 1-second segments back from the newest (109): 104.
        assert_eq!(Media::parse(&text, &base).start(), 104);
        // Fewer than that listed: all of them.
        let few = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:1,\na.ts\n#EXTINF:1,\nb.ts\n";
        assert_eq!(Media::parse(few, &base).start(), 7);
    }
}
