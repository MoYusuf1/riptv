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

use std::{
    collections::HashMap,
    process::Stdio,
    sync::Mutex,
    time::{Duration, SystemTime},
};

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
    time::{Instant, sleep, timeout, timeout_at},
};
use tokio_util::io::ReaderStream;
use xtream::sniff::Plan;

use crate::{
    AppState,
    compat::{
        MISSING, Piped, agent, encode_args, hardware_encoder, max_height, pick_variant, threads,
        vet, watch_ffmpeg,
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
    /// Seconds behind the live edge to begin (more buffer against a late segment), when more
    /// than [`START_SECONDS`]; capped at two minutes.
    back: Option<u32>,
}

/// How much of the stream is read, at most, before deciding: the program map comes in the first
/// packets, but the first keyframe (where the picture can begin) can be a few seconds in when a
/// provider cuts segments mid-GOP.
const SNIFF_MAX: usize = 4 * 1024 * 1024;
/// Chunks in flight between the provider and ffmpeg.
const QUEUE: usize = 32;
/// A queue slot owns at most this many bytes (32 slots = 2 MiB, regardless of upstream chunks).
const QUEUED_BYTES: usize = 64 * 1024;

async fn enqueue(tx: &mpsc::Sender<Bytes>, bytes: Bytes) -> Result<(), ()> {
    if bytes.len() <= QUEUED_BYTES {
        return tx.send(bytes).await.map_err(|_| ());
    }
    for part in bytes.chunks(QUEUED_BYTES) {
        // Own the small allocation: slicing alone would retain the large parent buffer.
        tx.send(Bytes::copy_from_slice(part))
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}
/// A playlist this large is not a playlist.
const PLAYLIST_MAX: usize = 1024 * 1024;
const READ_IDLE: Duration = Duration::from_secs(10);
const REQUEST_WAIT: Duration = Duration::from_secs(10);
/// How long a playlist refresh may take before the link counts as dead.
const PLAYLIST_WAIT: Duration = Duration::from_secs(4);

/// Provider-wide cooldowns also cover failures after response headers were sent. No URLs,
/// account names, or passwords are retained. Expired entries are removed on access.
#[derive(Default)]
pub(crate) struct Limits(Mutex<LimitState>);
#[derive(Default)]
struct LimitState {
    providers: HashMap<String, Instant>,
    overflow: Option<Instant>,
}
impl Limits {
    fn remaining(&self, url: &Url) -> u64 {
        let mut limits = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        limits.providers.retain(|_, until| *until > now);
        let overflow = limits.overflow.filter(|until| *until > now);
        if limits.providers.is_empty() && overflow.is_none() {
            return 0;
        }
        limits
            .providers
            .get(&url.origin().ascii_serialization())
            .copied()
            .into_iter()
            .chain(overflow)
            .max()
            .map_or(0, |until| {
                until
                    .saturating_duration_since(Instant::now())
                    .as_secs()
                    .saturating_add(1)
            })
    }
    fn set(&self, url: &Url, seconds: u64) {
        if seconds == 0 {
            return;
        }
        let mut limits = self.0.lock().unwrap_or_else(|e| e.into_inner());
        let now = Instant::now();
        limits.providers.retain(|_, until| *until > now);
        let key = url.origin().ascii_serialization();
        let until = now + Duration::from_secs(seconds.min(7 * 24 * 3600));
        // At capacity, conservatively wait for all providers. Never drop an active limit.
        if limits.providers.len() >= 64 && !limits.providers.contains_key(&key) {
            limits.overflow = Some(limits.overflow.map_or(until, |old| old.max(until)));
            return;
        }
        limits
            .providers
            .entry(key)
            .and_modify(|old| *old = (*old).max(until))
            .or_insert(until);
    }
    fn carry(&self, from: &Url, to: &Url) {
        self.set(to, self.remaining(from));
    }
}

fn retry_seconds(value: &str, now: SystemTime) -> Option<u64> {
    value
        .trim()
        .parse::<u64>()
        .ok()
        .or_else(|| {
            httpdate::parse_http_date(value)
                .ok()
                .map(|date| date.duration_since(now).unwrap_or_default().as_secs())
        })
        .map(|seconds| seconds.min(7 * 24 * 3600))
}

fn limited(seconds: u64, code: u16) -> Response {
    let why = if code > 0 {
        format!("Server returned {code}. The provider asked us to wait before retrying.")
    } else {
        "The provider asked us to wait before retrying.".into()
    };
    let mut response = refusal(StatusCode::TOO_MANY_REQUESTS, &why);
    if let Ok(value) = seconds.to_string().parse() {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

fn interrupted(s: &AppState, url: &Url, status: StatusCode, why: &str) -> Response {
    let wait = s.live_limits.remaining(url);
    if wait > 0 {
        limited(wait, 0)
    } else {
        refusal(status, why)
    }
}

#[derive(Debug)]
enum FetchError {
    Request(String),
    Status(u16),
    Limited(u64, u16),
}

impl From<String> for FetchError {
    fn from(why: String) -> Self {
        Self::Request(why)
    }
}
impl From<&str> for FetchError {
    fn from(why: &str) -> Self {
        Self::Request(why.into())
    }
}

impl FetchError {
    fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Request(_) | Self::Status(408 | 429 | 500 | 502 | 503 | 504)
        )
    }
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(why) => f.write_str(why),
            Self::Status(code) => write!(f, "Server returned {code}"),
            Self::Limited(seconds, code) => write!(
                f,
                "Server returned {code}; provider requested a {seconds}s cooldown"
            ),
        }
    }
}

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
    let wait = s.live_limits.remaining(&url);
    if wait > 0 {
        return limited(wait, 0);
    }
    let tag = diagnostics::tag(&s, &url);
    let started = Instant::now();

    // The provider's bytes, in order, from a task that follows the channel.
    let (tx, mut rx) = mpsc::channel::<Bytes>(QUEUE);
    // Noted before asking, so a provider too slow to answer at all still shows in the log.
    diagnostics::note(&s, &format!("{tag}live asking the provider"));
    let mut steps = vec![];
    let source = match Source::open(&s, &url, &ua, &mut steps).await {
        Ok(Source::Hls {
            original,
            playlist,
            media,
            next: _,
        }) => {
            let back = q.back.map_or(START_SECONDS, |b| f64::from(b.min(120)));
            let next = media.start(back);
            Source::Hls {
                original,
                playlist,
                media,
                next,
            }
        }
        Ok(source) => source,
        Err(why) => {
            diagnostics::note(&s, &format!("{tag}live could not start: {why}"));
            let wait = s.live_limits.remaining(&url);
            if wait > 0 {
                return limited(
                    wait,
                    match &why {
                        FetchError::Limited(_, code) => *code,
                        _ => 0,
                    },
                );
            }
            return refusal(
                match &why {
                    FetchError::Status(code @ (401 | 403 | 404 | 410)) => {
                        StatusCode::from_u16(*code).unwrap_or(StatusCode::BAD_GATEWAY)
                    }
                    _ => StatusCode::BAD_GATEWAY,
                },
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
        Ok(None) => {
            return interrupted(
                &s,
                &url,
                StatusCode::BAD_GATEWAY,
                "the stream ended before it began",
            );
        }
        Err(_) => {
            return interrupted(
                &s,
                &url,
                StatusCode::GATEWAY_TIMEOUT,
                "the provider sent nothing for 15 s",
            );
        }
    };
    let mut inspector = xtream::sniff::GrowingSniff::default();
    let sniffed = loop {
        let found = inspector.update(&head);
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
    let encoder = if transcode {
        hardware_encoder().await
    } else {
        None
    };
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
    args.extend(["-threads".into(), threads(), "-i".into(), "pipe:0".into()]);
    args.extend(encode_args(
        video,
        encoder,
        sniffed.height.unwrap_or(0),
        max_height(),
        // Deinterlaced unless the stream reader saw a progressive picture.
        sniffed.interlaced != Some(false),
        audio,
    ));
    let mut child = match Command::new(crate::compat::media_tool("ffmpeg"))
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
            return interrupted(
                &s,
                &url,
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
        .header(
            "x-riptv-video",
            if sniffed.video.is_none() {
                "none"
            } else if video == "copy" && sniffed.video == Some("hevc") {
                "hevc"
            } else {
                "h264"
            },
        )
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
        original: Url,
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
    ) -> Result<Self, FetchError> {
        let mut current = url.clone();
        for _ in 0..2 {
            let asked = Instant::now();
            let mut response = match get_retry(s, &current, ua, "", "opening stream").await {
                Ok(response) => response,
                Err(error) => {
                    s.live_limits.carry(&current, url);
                    return Err(error);
                }
            };
            let redirected = response.url() != &current;
            let base = response.url().clone();
            let first = response.chunk();
            let mut first = timeout(READ_IDLE, first)
                .await
                .map_err(|_| "the provider stopped sending data".to_string())?
                .map_err(|_| "the connection broke".to_string())?
                .unwrap_or_default();
            // HTTP chunks need not contain a full playlist signature. Only accumulate while
            // the bytes could still be that signature; transport streams start immediately.
            let deadline = Instant::now() + READ_IDLE;
            while first.len() < 7 && b"#EXTM3U".starts_with(&first) {
                let Some(chunk) = timeout_at(deadline, response.chunk())
                    .await
                    .map_err(|_| "the provider stopped sending data".to_string())?
                    .map_err(|_| "the connection broke".to_string())?
                else {
                    break;
                };
                if first.len().saturating_add(chunk.len()) > PLAYLIST_MAX {
                    return Err("the playlist is too large".into());
                }
                let mut joined = first.to_vec();
                joined.extend_from_slice(&chunk);
                first = Bytes::from(joined);
            }
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
            let next = media.start(START_SECONDS);
            return Ok(Self::Hls {
                original: url.clone(),
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
                if enqueue(&tx, first).await.is_err() {
                    return;
                }
                while let Ok(Ok(Some(chunk))) = timeout(READ_IDLE, response.chunk()).await {
                    if enqueue(&tx, chunk).await.is_err() {
                        return;
                    }
                }
                diagnostics::note(&s, &format!("{tag}live stream ended by the provider"));
            }
            Self::Hls {
                original,
                mut playlist,
                mut media,
                mut next,
            } => {
                // A link that dies for good is reopened here (three tries, 1+2+4 s) and continued
                // from the next segment: the converter and the browser never see it end. A
                // provider that keeps refusing, or a list we can't continue, ends the response
                // so the app can try its other routes.
                loop {
                    (_, media, next) =
                        follow(&s, &ua, &tag, &tx, &original, playlist, media, next).await;
                    if media.ended || tx.is_closed() {
                        return;
                    }
                    let mut reopened = None;
                    for attempt in 0..3 {
                        sleep(Duration::from_secs(1 << attempt)).await;
                        reopened = renew(&s, &original, &ua, &media, next).await;
                        if reopened.is_some() || tx.is_closed() {
                            break;
                        }
                    }
                    let Some((fresh, updated, at)) = reopened else {
                        diagnostics::note(
                            &s,
                            &format!("{tag}live couldn't reopen the provider's link"),
                        );
                        return;
                    };
                    diagnostics::note(
                        &s,
                        &format!(
                            "{tag}live reopened the provider's link; continuing at segment {at}"
                        ),
                    );
                    (playlist, media, next) = (fresh, updated, at);
                }
            }
        }
    }
}

/// Follows an HLS media playlist from `next`, renewing a stale link on the way; returns where it
/// stopped (the playlist, its list and the next segment) when it can go no further.
#[allow(clippy::too_many_arguments)]
async fn follow(
    s: &AppState,
    ua: &str,
    tag: &str,
    tx: &mpsc::Sender<Bytes>,
    original: &Url,
    mut playlist: Url,
    mut media: Media,
    mut next: u64,
) -> (Url, Media, u64) {
    let mut quiet_since = Instant::now();
    let mut refreshed = Instant::now();
    let mut skipped = 0_u8;
    let mut renewed_at = None::<Instant>;
    let mut changed = true;
    loop {
        let from = next;
        for (seq, segment, length) in media.segments.iter().filter(|(seq, ..)| *seq >= from) {
            let started = Instant::now();
            let mut response = match get_retry(s, segment, ua, tag, "segment").await {
                Ok(response) => response,
                Err(FetchError::Status(404 | 410)) if skipped < 2 => {
                    // Only skip a missing segment before forwarding any of its bytes.
                    // Two skips per connection, not an unlimited hole in the timeline.
                    skipped += 1;
                    next = seq.saturating_add(1);
                    diagnostics::note(s, &format!("{tag}live missing segment {seq}; skipping"));
                    continue;
                }
                Err(why) => {
                    s.live_limits.carry(segment, original);
                    diagnostics::note(
                        s,
                        &format!("{tag}live segment {seq} failed: {why}; reopening stream"),
                    );
                    return (playlist, media, next);
                }
            };
            let mut bytes = 0_u64;
            let mut blocked = Duration::ZERO;
            loop {
                let chunk = match timeout(READ_IDLE, response.chunk()).await {
                    Ok(Ok(Some(chunk))) => chunk,
                    Ok(Ok(None)) => break,
                    _ => {
                        // Never retry a partially forwarded segment: that would repeat
                        // transport timestamps and corrupt the browser's decoder.
                        diagnostics::note(
                            s,
                            &format!(
                                "{tag}live segment {seq} interrupted after {bytes} bytes; reopening stream"
                            ),
                        );
                        return (playlist, media, next);
                    }
                };
                bytes += chunk.len() as u64;
                let sending = Instant::now();
                if enqueue(tx, chunk).await.is_err() {
                    return (playlist, media, next); // the viewer left
                }
                blocked += sending.elapsed();
            }
            let took = started.elapsed().as_secs_f64();
            let duration = if *length > 0.0 { *length } else { media.target };
            let network = (started.elapsed().saturating_sub(blocked)).as_secs_f64();
            diagnostics::note(
                s,
                &format!(
                    "{tag}live segment {seq} delivery bytes={bytes} video_s={duration:.1} elapsed_s={took:.2} downstream_wait_s={:.2} upstream_s={network:.2}",
                    blocked.as_secs_f64()
                ),
            );
            if network > duration {
                diagnostics::note(
                    s,
                    &format!(
                        "{tag}live segment {seq} took {network:.1}s for {duration:.1}s of video upstream"
                    ),
                );
            }
            let Some(after) = seq.checked_add(1) else {
                return (playlist, media, next);
            };
            next = after;
            quiet_since = Instant::now();
        }
        if media.ended {
            return (playlist, media, next);
        }
        let quiet = quiet_since.elapsed().as_secs_f64();
        // The provider's redirected link can go stale (one real provider's does after
        // 2 minutes) and keep serving the old list: ask the original address for a
        // fresh one while the browser's buffer still covers the gap.
        let may_renew =
            playlist != *original && renewed_at.is_none_or(|t| t.elapsed() >= RENEW_GAP);
        if quiet > (media.target * 1.2).max(4.0) && may_renew {
            renewed_at = Some(Instant::now());
            if let Some((fresh, updated, at)) = renew(s, original, ua, &media, next).await {
                diagnostics::note(s, &format!("{tag}live playlist stopped advancing; renewed"));
                (playlist, media, next, changed) = (fresh, updated, at, true);
                continue;
            }
            diagnostics::note(
                s,
                &format!("{tag}live playlist stopped advancing; renewal didn't continue it"),
            );
        }
        if quiet > (media.target * 2.0).clamp(10.0, 20.0) {
            diagnostics::note(s, &format!("{tag}live playlist stopped advancing"));
            return (playlist, media, next);
        }
        // Changed playlists wait a full target; unchanged ones half (RFC 8216).
        let wait =
            Duration::from_secs_f64((media.target / if changed { 1.0 } else { 2.0 }).max(0.5));
        if let Some(left) = wait.checked_sub(refreshed.elapsed()) {
            sleep(left).await;
        }
        if tx.is_closed() {
            return (playlist, media, next);
        }
        refreshed = Instant::now();
        // One short try, not three long ones: a refresh answers in well under a second, and
        // a link that hangs is renewed below (a real one hung 33 s and the buffer fell to 5 s).
        let refresh = timeout(PLAYLIST_WAIT, get(s, &playlist, ua))
            .await
            .unwrap_or_else(|_| Err(FetchError::Request("the provider didn't answer".into())));
        match refresh {
            Ok(response) => match read_rest(response, Bytes::new()).await {
                Ok(text) => {
                    let updated = Media::parse(&text, &playlist);
                    changed = updated.sequence != media.sequence
                        || updated.segments.last().map(|s| s.0)
                            != media.segments.last().map(|s| s.0);
                    media = updated;
                }
                Err(why) => {
                    diagnostics::note(
                        s,
                        &format!("{tag}live playlist refresh: {why}; reopening stream"),
                    );
                    return (playlist, media, next);
                }
            },
            // A short cooldown is waited out while the buffer covers it; the
            // "stopped advancing" limit above still ends a provider that never lets up.
            Err(FetchError::Limited(wait, _)) if wait <= 10 => {
                sleep(Duration::from_secs(wait)).await;
            }
            Err(why) => {
                s.live_limits.carry(&playlist, original);
                // Renew an expired or unanswered redirect/master link (a real
                // provider's answers 407 every 2 minutes), keeping the next sequence
                // number so already forwarded segments cannot be replayed.
                if matches!(why, FetchError::Request(_) | FetchError::Status(_))
                    && playlist != *original
                    && renewed_at.is_none_or(|t| t.elapsed() >= RENEW_GAP)
                {
                    renewed_at = Some(Instant::now());
                    if let Some((fresh, updated, at)) = renew(s, original, ua, &media, next).await {
                        diagnostics::note(
                            s,
                            &format!("{tag}live playlist refresh: {why}; renewed"),
                        );
                        (playlist, media, next, changed) = (fresh, updated, at, true);
                        continue;
                    }
                }
                diagnostics::note(
                    s,
                    &format!("{tag}live playlist refresh: {why}; reopening stream"),
                );
                return (playlist, media, next);
            }
        }
    }
}

async fn get(s: &AppState, url: &Url, ua: &str) -> Result<reqwest::Response, FetchError> {
    let wait = s.live_limits.remaining(url);
    if wait > 0 {
        return Err(FetchError::Limited(wait, 429));
    }
    let request = s
        .http
        .get(url.clone())
        .header(header::USER_AGENT, ua)
        .send();
    let response = timeout(REQUEST_WAIT, request)
        .await
        .map_err(|_| FetchError::Request("the provider didn't answer".into()))?
        .map_err(|e| {
            FetchError::Request(if e.is_timeout() {
                "the provider didn't answer".to_string()
            } else {
                "could not connect to the provider".to_string()
            })
        })?;
    let status = response.status();
    if !status.is_success() {
        if let Some(seconds) = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| retry_seconds(v, SystemTime::now()))
            .filter(|s| *s > 0)
        {
            s.live_limits.set(url, seconds);
            return Err(FetchError::Limited(seconds, status.as_u16()));
        }
        if matches!(status.as_u16(), 429 | 509) {
            s.live_limits.set(url, 5);
            return Err(FetchError::Limited(5, status.as_u16()));
        }
        return Err(FetchError::Status(status.as_u16()));
    }
    Ok(response)
}

/// Retry only before any body bytes have been forwarded, serially and with backoff.
/// Refusals such as 509 require reopening the original URL, not hammering its stale redirect.
async fn get_retry(
    s: &AppState,
    url: &Url,
    ua: &str,
    tag: &str,
    what: &str,
) -> Result<reqwest::Response, FetchError> {
    for attempt in 0..3 {
        match get(s, url, ua).await {
            Ok(response) => return Ok(response),
            Err(error) if error.retryable() && attempt < 2 => {
                diagnostics::note(
                    s,
                    &format!("{tag}live {what}: {error}; retry {}", attempt + 1),
                );
                sleep(Duration::from_millis(500 * (attempt + 1))).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

async fn read_rest(mut response: reqwest::Response, first: Bytes) -> Result<String, String> {
    if first.len() > PLAYLIST_MAX {
        return Err("the playlist is too large".into());
    }
    let mut body = first.to_vec();
    let deadline = Instant::now() + READ_IDLE;
    while body.len() < PLAYLIST_MAX {
        match timeout_at(deadline, response.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                if body.len().saturating_add(chunk.len()) > PLAYLIST_MAX {
                    return Err("the playlist is too large".into());
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Ok(None)) => break,
            _ => return Err("the connection broke or stopped sending data".into()),
        }
    }
    if body.len() >= PLAYLIST_MAX {
        return Err("the playlist is too large".into());
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Seconds of video, already on the server, to start from: all of it arrives at once, so ffmpeg
/// sets up and the browser buffers without waiting on real time (1-second segments otherwise
/// trickle in), with a cushion against the next segment coming late.
const START_SECONDS: f64 = 18.0;

/// Least time between asking the original link for a fresh playlist: a renewed link can die
/// within seconds too, so renewal isn't once-only, but the provider isn't hammered either.
const RENEW_GAP: Duration = Duration::from_secs(3);
/// Segments a renewed playlist may have moved past ours and still be followed (a gap the player
/// skips) rather than reopened.
const RESUME_SKIP: u64 = 3;

/// Where a renewed link continues the stream: its playlist, list and next segment.
async fn renew(
    s: &AppState,
    original: &Url,
    ua: &str,
    media: &Media,
    next: u64,
) -> Option<(Url, Media, u64)> {
    let Ok(Source::Hls {
        playlist,
        media: fresh,
        ..
    }) = Source::open(s, original, ua, &mut vec![]).await
    else {
        return None;
    };
    let at = fresh.resume_at(media, next)?;
    Some((playlist, fresh, at))
}

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
    /// Where a renewed playlist picks up after `next - 1`, or `None` if it may be another
    /// timeline. The last forwarded segment, same number and file name, continues exactly; with
    /// it already gone, the first of the next few numbers (a short gap the player skips), unless
    /// that number names another file. Only the file name: a renewed link puts a new token in the
    /// rest, and at the live edge the next segment isn't listed yet.
    fn resume_at(&self, old: &Self, next: u64) -> Option<u64> {
        if self.continues(old, next) {
            return Some(next);
        }
        let previous = next.checked_sub(1)?;
        if self.segments.iter().any(|(seq, _, _)| *seq == previous) {
            return None;
        }
        self.segments
            .iter()
            .map(|(seq, _, _)| *seq)
            .find(|seq| (next..=next + RESUME_SKIP).contains(seq))
    }

    fn continues(&self, old: &Self, next: u64) -> bool {
        let Some(previous) = next.checked_sub(1) else {
            return false;
        };
        let file = |u: &Url| {
            u.path_segments()
                .and_then(|mut p| p.next_back())
                .map(str::to_owned)
        };
        old.segments
            .iter()
            .find(|(seq, _, _)| *seq == previous)
            .zip(self.segments.iter().find(|(seq, _, _)| *seq == previous))
            .is_some_and(|(old, new)| file(&old.1).is_some() && file(&old.1) == file(&new.1))
    }

    fn parse(text: &str, base: &Url) -> Self {
        let tag = |name: &str| {
            text.lines()
                .find_map(|l| l.trim().strip_prefix(name))
                .map(str::trim)
        };
        let sequence: u64 = tag("#EXT-X-MEDIA-SEQUENCE:")
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
                segments.push((sequence.saturating_add(segments.len() as u64), url, seconds));
            }
        }
        Self {
            target: tag("#EXT-X-TARGETDURATION:")
                .and_then(|v| v.parse::<f64>().ok())
                .filter(|v| v.is_finite())
                .unwrap_or(6.0_f64)
                .clamp(1.0, 60.0),
            sequence,
            segments,
            ended: text.contains("#EXT-X-ENDLIST"),
        }
    }
}

impl Media {
    /// Where to begin: at least three segments (the HLS spec's minimum distance from the end,
    /// what ExoPlayer does) and `back` seconds (at least [`START_SECONDS`]) from the newest.
    /// With 10 s segments two left the buffer swinging down to 2 s between arrivals.
    fn start(&self, back: f64) -> u64 {
        let back = back.max(START_SECONDS);
        let mut seconds = 0.0;
        for (taken, (seq, _, length)) in self.segments.iter().rev().enumerate() {
            seconds += if *length > 0.0 { *length } else { self.target };
            if taken >= 2 && seconds >= back {
                return *seq;
            }
        }
        self.segments.first().map_or(self.sequence, |s| s.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::get as route_get};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    async fn provider(router: Router) -> (Url, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        (base, task)
    }

    fn test_state() -> AppState {
        crate::tls();
        AppState {
            http: reqwest::Client::new(),
            ..AppState::new()
        }
    }

    #[test]
    fn retry_after_accepts_seconds_and_http_dates() {
        let now = httpdate::parse_http_date("Wed, 21 Oct 2015 07:28:00 GMT").unwrap();
        assert_eq!(retry_seconds("30", now), Some(30));
        assert_eq!(
            retry_seconds("Wed, 21 Oct 2015 07:28:30 GMT", now),
            Some(30)
        );
        assert_eq!(retry_seconds("Wed, 21 Oct 2015 07:27:30 GMT", now), Some(0));
        assert_eq!(retry_seconds("invalid", now), None);
    }

    #[test]
    fn cooldown_storage_is_bounded_and_never_evicts_active_limits() {
        let limits = Limits::default();
        for id in 0..100 {
            limits.set(
                &Url::parse(&format!("https://provider-{id}.example/")).unwrap(),
                30,
            );
        }
        assert_eq!(limits.0.lock().unwrap().providers.len(), 64);
        assert!(limits.remaining(&Url::parse("https://provider-99.example/").unwrap()) >= 29);
        assert!(limits.remaining(&Url::parse("https://provider-0.example/").unwrap()) >= 29);
    }

    #[tokio::test]
    async fn segment_rate_limit_survives_headers_already_sent_to_the_browser() {
        let (base, task) = provider(Router::new().route(
            "/segment",
            route_get(|| async {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, "30")],
                    "",
                )
            }),
        ))
        .await;
        let original = Url::parse("https://original.example/live").unwrap();
        let state = test_state();
        let (tx, mut rx) = mpsc::channel(32);
        Source::Hls {
            original: original.clone(),
            playlist: base.clone(),
            media: Media::parse("#EXTM3U\n#EXTINF:1,\nsegment\n#EXT-X-ENDLIST\n", &base),
            next: 0,
        }
        .run(state.clone(), String::new(), String::new(), tx)
        .await;
        assert!(rx.recv().await.is_none());
        // This returns before DNS or a network request to the original host.
        assert!(matches!(
            get(&state, &original, "").await,
            Err(FetchError::Limited(..))
        ));
        assert!(state.live_limits.remaining(&original) >= 29);
        task.abort();
    }

    #[tokio::test]
    async fn split_playlist_signature_is_not_mistaken_for_video() {
        struct Split {
            step: u8,
            delay: std::pin::Pin<Box<tokio::time::Sleep>>,
        }
        impl futures_core::Stream for Split {
            type Item = std::io::Result<Bytes>;
            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                match self.step {
                    0 => {
                        self.step = 1;
                        std::task::Poll::Ready(Some(Ok(Bytes::from_static(b"#EXT"))))
                    }
                    1 => {
                        if std::future::Future::poll(self.delay.as_mut(), cx).is_pending() {
                            return std::task::Poll::Pending;
                        }
                        self.step = 2;
                        std::task::Poll::Ready(Some(Ok(Bytes::from_static(
                            b"M3U\n#EXTINF:1,\nsegment\n#EXT-X-ENDLIST\n",
                        ))))
                    }
                    _ => std::task::Poll::Ready(None),
                }
            }
        }
        let (base, task) = provider(Router::new().route(
            "/",
            route_get(|| async {
                Body::from_stream(Split {
                    step: 0,
                    delay: Box::pin(sleep(Duration::from_millis(50))),
                })
            }),
        ))
        .await;
        assert!(matches!(
            Source::open(&test_state(), &base, "", &mut vec![]).await,
            Ok(Source::Hls { .. })
        ));
        task.abort();
    }

    #[tokio::test]
    async fn retry_after_reaches_the_browser_and_blocks_manual_reopens() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (base, task) = provider(Router::new().route(
            "/",
            route_get(move || {
                count.fetch_add(1, Ordering::SeqCst);
                async {
                    (
                        StatusCode::SERVICE_UNAVAILABLE,
                        [(header::RETRY_AFTER, "30")],
                        "wait",
                    )
                }
            }),
        ))
        .await;
        let s = test_state();
        s.approved.write().unwrap().insert("127.0.0.1".into());
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:3000".parse().unwrap());
        headers.insert("sec-fetch-site", "same-origin".parse().unwrap());
        for _ in 0..3 {
            let response = stream(
                State(s.clone()),
                Query(LiveQuery {
                    url: base.to_string(),
                    can: String::new(),
                    video: None,
                    back: None,
                }),
                headers.clone(),
            )
            .await;
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
            assert!(
                response.headers()[header::RETRY_AFTER]
                    .to_str()
                    .unwrap()
                    .parse::<u64>()
                    .unwrap()
                    >= 29
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
    }

    #[tokio::test]
    async fn redirected_playlist_cooldown_applies_to_the_original_url() {
        let (base, task) = provider(
            Router::new()
                .route(
                    "/",
                    route_get(|| async { "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000\nvariant\n" }),
                )
                .route(
                    "/variant",
                    route_get(|| async {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [(header::RETRY_AFTER, "20")],
                            "",
                        )
                    }),
                ),
        )
        .await;
        let s = test_state();
        assert!(matches!(
            Source::open(&s, &base, "", &mut vec![]).await,
            Err(FetchError::Limited(..))
        ));
        assert!(s.live_limits.remaining(&base) >= 19);
        task.abort();
    }

    #[tokio::test]
    async fn first_segment_cooldown_is_not_hidden_by_a_gateway_error() {
        let (base, task) = provider(
            Router::new()
                .route(
                    "/",
                    route_get(|| async { "#EXTM3U\n#EXTINF:1,\nsegment\n#EXT-X-ENDLIST\n" }),
                )
                .route(
                    "/segment",
                    route_get(|| async {
                        (
                            StatusCode::TOO_MANY_REQUESTS,
                            [(header::RETRY_AFTER, "30")],
                            "",
                        )
                    }),
                ),
        )
        .await;
        let state = test_state();
        state.approved.write().unwrap().insert("127.0.0.1".into());
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "127.0.0.1:3000".parse().unwrap());
        let response = stream(
            State(state),
            Query(LiveQuery {
                url: base.to_string(),
                can: String::new(),
                video: None,
                back: None,
            }),
            headers,
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key(header::RETRY_AFTER));
        task.abort();
    }

    #[tokio::test]
    async fn missing_segments_are_skipped_but_only_twice() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (base, task) = provider(
            Router::new()
                .route(
                    "/gone",
                    route_get(move || {
                        count.fetch_add(1, Ordering::SeqCst);
                        async { StatusCode::NOT_FOUND }
                    }),
                )
                .route("/good", route_get(|| async { "good" })),
        )
        .await;
        for (playlist, expected) in [
            (
                "#EXTM3U\n#EXTINF:1,\ngone\n#EXTINF:1,\ngood\n#EXT-X-ENDLIST\n",
                b"good".as_slice(),
            ),
            (
                "#EXTM3U\n#EXTINF:1,\ngone\n#EXTINF:1,\ngone\n#EXTINF:1,\ngone\n#EXTINF:1,\ngood\n#EXT-X-ENDLIST\n",
                b"".as_slice(),
            ),
        ] {
            let (tx, mut rx) = mpsc::channel(32);
            Source::Hls {
                original: base.clone(),
                playlist: base.clone(),
                media: Media::parse(playlist, &base),
                next: 0,
            }
            .run(test_state(), String::new(), String::new(), tx)
            .await;
            let mut data = vec![];
            while let Some(chunk) = rx.recv().await {
                data.extend_from_slice(&chunk);
            }
            assert_eq!(data, expected);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 4);
        task.abort();
    }

    #[tokio::test]
    async fn expired_variant_is_renewed_without_replaying_consumed_segments() {
        let segment_calls = Arc::new(AtomicUsize::new(0));
        let count = segment_calls.clone();
        let (base, task) = provider(
            Router::new()
                .route(
                    "/",
                    route_get(|| async {
                        "#EXTM3U\n#EXTINF:1,\none\n#EXTINF:1,\ntwo\n#EXT-X-ENDLIST\n"
                    }),
                )
                .route("/expired", route_get(|| async { StatusCode::GONE }))
                .route(
                    "/token",
                    route_get(|| async { StatusCode::PROXY_AUTHENTICATION_REQUIRED }),
                )
                .route(
                    "/one",
                    route_get(move || {
                        count.fetch_add(1, Ordering::SeqCst);
                        async { "one" }
                    }),
                )
                .route("/two", route_get(|| async { "two" })),
        )
        .await;
        for (n, link) in ["expired", "token"].into_iter().enumerate() {
            let (tx, mut rx) = mpsc::channel(32);
            Source::Hls {
                original: base.clone(),
                playlist: base.join(link).unwrap(),
                media: Media::parse("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none\n", &base),
                next: 0,
            }
            .run(test_state(), String::new(), String::new(), tx)
            .await;
            let mut data = vec![];
            while let Some(chunk) = rx.recv().await {
                data.extend_from_slice(&chunk);
            }
            assert_eq!(data, b"onetwo", "{link}");
            assert_eq!(segment_calls.load(Ordering::SeqCst), n + 1, "{link}");
        }
        task.abort();
    }

    #[tokio::test]
    async fn a_stale_redirected_playlist_is_renewed_from_the_original_link() {
        let (base, task) = provider(
            Router::new()
                .route(
                    "/",
                    route_get(|| async {
                        "#EXTM3U\n#EXTINF:1,\ntok2/one\n#EXTINF:1,\ntok2/two\n#EXT-X-ENDLIST\n"
                    }),
                )
                .route(
                    "/stale",
                    route_get(|| async { "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none\n" }),
                )
                .route("/one", route_get(|| async { "one" }))
                .route("/tok2/two", route_get(|| async { "two" })),
        )
        .await;
        let (tx, mut rx) = mpsc::channel(32);
        Source::Hls {
            original: base.clone(),
            playlist: base.join("stale").unwrap(),
            media: Media::parse("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none\n", &base),
            next: 0,
        }
        .run(test_state(), String::new(), String::new(), tx)
        .await;
        let mut data = vec![];
        while let Some(chunk) = rx.recv().await {
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data, b"onetwo", "continued without replaying \"one\"");
        task.abort();
    }

    #[tokio::test]
    async fn a_dead_link_is_reopened_and_continued_without_gaps_or_repeats() {
        let fails = Arc::new(AtomicUsize::new(0));
        let count = fails.clone();
        let (base, task) = provider(
            Router::new()
                .route(
                    "/",
                    route_get(|| async {
                        "#EXTM3U\n#EXTINF:1,\none\n#EXTINF:1,\ntwo\n#EXTINF:1,\nthree\n#EXT-X-ENDLIST\n"
                    }),
                )
                .route("/one", route_get(|| async { "one" }))
                .route(
                    "/two",
                    route_get(move || {
                        let failing = count.fetch_add(1, Ordering::SeqCst) < 3;
                        async move {
                            if failing {
                                (StatusCode::INTERNAL_SERVER_ERROR, "")
                            } else {
                                (StatusCode::OK, "two")
                            }
                        }
                    }),
                )
                .route("/three", route_get(|| async { "three" })),
        )
        .await;
        let (tx, mut rx) = mpsc::channel(32);
        Source::Hls {
            original: base.clone(),
            playlist: base.clone(),
            media: Media::parse(
                "#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none\n#EXTINF:1,\ntwo\n",
                &base,
            ),
            next: 0,
        }
        .run(test_state(), String::new(), String::new(), tx)
        .await;
        let mut data = vec![];
        while let Some(chunk) = rx.recv().await {
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data, b"onetwothree");
        task.abort();
    }

    #[tokio::test]
    async fn a_hanging_playlist_link_is_renewed_within_seconds() {
        let (base, task) = provider(
            Router::new()
                .route(
                    "/",
                    route_get(|| async {
                        "#EXTM3U\n#EXTINF:1,\none\n#EXTINF:1,\ntwo\n#EXT-X-ENDLIST\n"
                    }),
                )
                .route(
                    "/hang",
                    route_get(|| async {
                        sleep(Duration::from_secs(60)).await;
                        ""
                    }),
                )
                .route("/one", route_get(|| async { "one" }))
                .route("/two", route_get(|| async { "two" })),
        )
        .await;
        let (tx, mut rx) = mpsc::channel(32);
        let started = Instant::now();
        Source::Hls {
            original: base.clone(),
            playlist: base.join("hang").unwrap(),
            media: Media::parse("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none\n", &base),
            next: 0,
        }
        .run(test_state(), String::new(), String::new(), tx)
        .await;
        let mut data = vec![];
        while let Some(chunk) = rx.recv().await {
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data, b"onetwo");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "{:?}",
            started.elapsed()
        );
        task.abort();
    }

    #[tokio::test]
    async fn a_short_provider_cooldown_is_waited_out_not_ended() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (base, task) = provider(
            Router::new()
                .route(
                    "/busy",
                    route_get(move || {
                        let first = count.fetch_add(1, Ordering::SeqCst) == 0;
                        async move {
                            if first {
                                (StatusCode::from_u16(509).unwrap(), String::new())
                            } else {
                                (
                                    StatusCode::OK,
                                    "#EXTM3U\n#EXTINF:1,\none\n#EXTINF:1,\ntwo\n#EXT-X-ENDLIST\n"
                                        .to_string(),
                                )
                            }
                        }
                    }),
                )
                .route("/one", route_get(|| async { "one" }))
                .route("/two", route_get(|| async { "two" })),
        )
        .await;
        let (tx, mut rx) = mpsc::channel(32);
        let started = Instant::now();
        Source::Hls {
            original: base.clone(),
            playlist: base.join("busy").unwrap(),
            media: Media::parse("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXTINF:1,\none\n", &base),
            next: 0,
        }
        .run(test_state(), String::new(), String::new(), tx)
        .await;
        let mut data = vec![];
        while let Some(chunk) = rx.recv().await {
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data, b"onetwo");
        assert!(
            started.elapsed() >= Duration::from_secs(5),
            "the cooldown was respected"
        );
        task.abort();
    }

    #[test]
    fn renewal_rejects_a_different_timeline_and_invalid_target_duration() {
        let base = Url::parse("https://example.org/").unwrap();
        let old = Media::parse("#EXTM3U\n#EXTINF:1,\none\n", &base);
        let new = Media::parse(
            "#EXTM3U\n#EXT-X-TARGETDURATION:inf\n#EXTINF:1,\nother\n#EXTINF:1,\ntwo\n",
            &base,
        );
        assert!(!new.continues(&old, 1));
        assert_eq!(new.resume_at(&old, 1), None, "segment 0 names another file");
        // Ours (0, 1) already rolled off a renewed list: pick up just past them, but not far.
        let on = |first: u64| {
            Media::parse(
                &format!("#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:{first}\n#EXTINF:1,\na\n#EXTINF:1,\nb\n"),
                &base,
            )
        };
        assert_eq!(on(3).resume_at(&old, 2), Some(3));
        assert_eq!(on(9).resume_at(&old, 2), None, "too far ahead");
        assert_eq!(new.target, 6.0);
    }

    #[tokio::test]
    async fn temporary_segment_refusal_is_retried_without_skipping_or_duplicating() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (base, task) = provider(Router::new().route(
            "/segment",
            route_get(move || {
                let count = count.clone();
                async move {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        (StatusCode::SERVICE_UNAVAILABLE, "")
                    } else {
                        (StatusCode::OK, "complete segment")
                    }
                }
            }),
        ))
        .await;
        let s = AppState {
            http: {
                crate::tls();
                reqwest::Client::new()
            },
            ..AppState::new()
        };
        let media = Media::parse("#EXTM3U\n#EXTINF:1,\nsegment\n#EXT-X-ENDLIST\n", &base);
        let (tx, mut rx) = mpsc::channel(32);
        Source::Hls {
            original: base.clone(),
            playlist: base,
            media,
            next: 0,
        }
        .run(s, String::new(), String::new(), tx)
        .await;
        let mut data = Vec::new();
        while let Some(chunk) = rx.recv().await {
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data, b"complete segment");
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        task.abort();
    }

    #[tokio::test]
    async fn refused_playlist_ends_within_the_stall_limit_respecting_cooldowns() {
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (base, task) = provider(Router::new().route(
            "/",
            route_get(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    StatusCode::from_u16(509).unwrap()
                }
            }),
        ))
        .await;
        let s = AppState {
            http: {
                crate::tls();
                reqwest::Client::new()
            },
            ..AppState::new()
        };
        let media = Media::parse("#EXTM3U\n#EXT-X-TARGETDURATION:1\n", &base);
        let (tx, _rx) = mpsc::channel(32);
        // Each 509 asks for 5 s; those are waited out, but only until the playlist would count
        // as stopped (10 s here), then three reopen tries (7 s), never sooner than asked.
        timeout(
            Duration::from_secs(25),
            Source::Hls {
                original: base.clone(),
                playlist: base,
                media,
                next: 0,
            }
            .run(s, String::new(), String::new(), tx),
        )
        .await
        .expect("a provider that keeps refusing ends the source");
        assert!(calls.load(Ordering::SeqCst) <= 6, "{calls:?}");
        task.abort();
    }

    #[tokio::test]
    async fn broken_partial_segment_is_not_replayed_into_the_decoder() {
        struct BrokenSegment {
            sent: bool,
            delay: std::pin::Pin<Box<tokio::time::Sleep>>,
        }
        impl futures_core::Stream for BrokenSegment {
            type Item = std::io::Result<Bytes>;
            fn poll_next(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Option<Self::Item>> {
                if !self.sent {
                    self.sent = true;
                    return std::task::Poll::Ready(Some(Ok(Bytes::from_static(b"prefix"))));
                }
                if std::future::Future::poll(self.delay.as_mut(), cx).is_pending() {
                    return std::task::Poll::Pending;
                }
                std::task::Poll::Ready(Some(Err(std::io::Error::other("connection broke"))))
            }
        }
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let (base, task) = provider(Router::new().route(
            "/segment",
            route_get(move || {
                count.fetch_add(1, Ordering::SeqCst);
                async move {
                    Body::from_stream(BrokenSegment {
                        sent: false,
                        delay: Box::pin(sleep(Duration::from_millis(200))),
                    })
                }
            }),
        ))
        .await;
        let s = AppState {
            http: {
                crate::tls();
                reqwest::Client::new()
            },
            ..AppState::new()
        };
        let media = Media::parse("#EXTM3U\n#EXTINF:1,\nsegment\n#EXT-X-ENDLIST\n", &base);
        let (tx, mut rx) = mpsc::channel(32);
        Source::Hls {
            original: base.clone(),
            playlist: base,
            media,
            next: 0,
        }
        .run(s, String::new(), String::new(), tx)
        .await;
        let mut data = Vec::new();
        while let Some(chunk) = rx.recv().await {
            data.extend_from_slice(&chunk);
        }
        assert_eq!(data, b"prefix");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "partial segments must not be restarted in the same timeline"
        );
        task.abort();
    }

    #[test]
    fn retries_do_not_hammer_account_or_permanent_refusals() {
        for code in [401, 403, 404, 509] {
            assert!(!FetchError::Status(code).retryable());
        }
        for code in [408, 429, 500, 502, 503, 504] {
            assert!(FetchError::Status(code).retryable());
        }
    }

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
        // Only two 10 s segments listed: start at the older of the two.
        assert_eq!(m.start(START_SECONDS), 500);
    }

    #[test]
    fn short_segments_start_far_enough_back() {
        let base = Url::parse("https://h.tv/1.m3u8").unwrap();
        let mut text =
            String::from("#EXTM3U\n#EXT-X-TARGETDURATION:1\n#EXT-X-MEDIA-SEQUENCE:100\n");
        for i in 0..30 {
            text += &format!("#EXTINF:1.0,\n{i}.ts\n");
        }
        // Eighteen seconds of available history, without waiting on the live edge.
        assert_eq!(Media::parse(&text, &base).start(START_SECONDS), 112);
        // Asked for more (Smooth): further back; never less than the default.
        assert_eq!(Media::parse(&text, &base).start(25.0), 105);
        assert_eq!(Media::parse(&text, &base).start(5.0), 112);
        // Fewer than that listed: all of them.
        let few = "#EXTM3U\n#EXT-X-MEDIA-SEQUENCE:7\n#EXTINF:1,\na.ts\n#EXTINF:1,\nb.ts\n";
        assert_eq!(Media::parse(few, &base).start(START_SECONDS), 7);
    }
    #[tokio::test]
    async fn oversized_network_chunks_have_bounded_owned_queue_slots() {
        let input = Bytes::from(vec![42; QUEUED_BYTES * 3 + 17]);
        let expected = input.len();
        let (tx, mut rx) = mpsc::channel(2);
        let sending = tokio::spawn(async move {
            enqueue(&tx, input).await.unwrap();
        });
        let mut total = 0;
        while let Some(chunk) = rx.recv().await {
            assert!(chunk.len() <= QUEUED_BYTES);
            assert!(chunk.iter().all(|b| *b == 42));
            total += chunk.len();
        }
        sending.await.unwrap();
        assert_eq!(total, expected);
    }
}
