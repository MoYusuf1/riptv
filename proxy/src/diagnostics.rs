//! Opt-in local playback diagnostics (`riptv --logs`). Every channel or title the app opens gets a
//! session; the app reports what its player does (engine, stalls, errors, health) and the proxy
//! adds what it sees (upstream failures, slow segments, ffmpeg's speed and errors). When playback
//! fails or stalls, the proxy probes the stream itself. All of it goes to one log, one line per
//! fact, so a person or a local chatbot can read why a channel misbehaves as it happens.
//!
//! Credentials stay in this process's memory: the log, the responses and subprocess command
//! lines never carry a stream URL. Text that might contain one (an error message) is redacted.
//! Without `--logs` every endpoint here answers 404 and nothing is recorded.

use std::{
    collections::HashMap,
    fmt::Write as _,
    io::Write as _,
    path::Path as FsPath,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{AppState, from_app, url_ok};

/// A session lives this long after its last event.
const LIFE: Duration = Duration::from_secs(600);
const MAX_SESSIONS: usize = 16;
const SAMPLE_LIMIT: usize = xtream::sniff::SAMPLE;
/// At most one automatic probe per session in this long.
const PROBE_GAP: Duration = Duration::from_secs(60);
/// The event after which the stream is probed: the player has given up. Never while it might
/// still be connected: a provider that allows one connection per channel would end the
/// player's when the probe asks for the same stream.
const TROUBLE: [&str; 1] = ["failure"];

#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

#[derive(Clone)]
struct Session {
    url: Url,
    /// The browser's user agent: many providers refuse a request without a player-like one.
    agent: String,
    opened: Instant,
    expires: Instant,
    probed: Option<Instant>,
}

#[derive(Deserialize)]
pub struct Create {
    url: String,
}

#[derive(Serialize)]
struct Entry {
    id: String,
    seconds_left: u64,
}

fn clean(sessions: &mut HashMap<String, Session>) {
    let now = Instant::now();
    sessions.retain(|_, session| session.expires > now);
}

/// Wall-clock time of day (UTC) to the millisecond, for reading a log next to other logs.
fn stamp() -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let day = (ms / 1000) % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}Z",
        day / 3600,
        day / 60 % 60,
        day % 60,
        ms % 1000
    )
}

/// Appends one line to the log (and mirrors it to the terminal), if logging is on.
pub(crate) fn note(state: &AppState, line: &str) {
    if let Some(path) = &state.logs {
        write_line(path, line);
    }
}

fn write_line(path: &FsPath, line: &str) {
    let line = format!("{} {line}", stamp());
    eprintln!("{line}");
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(file, "{line}");
    }
}

/// The parts of a stream address that identify the account: user info, query values, the host,
/// and the username/password segments of an Xtream path (`/live/USER/PASS/123.ts`).
fn secrets(url: &Url) -> Vec<String> {
    let mut found = vec![url.username().to_owned()];
    found.extend(url.password().map(str::to_owned));
    found.extend(url.host_str().map(str::to_owned));
    found.extend(url.query_pairs().map(|(_, v)| v.into_owned()));
    let segments: Vec<&str> = url
        .path_segments()
        .map(Iterator::collect)
        .unwrap_or_default();
    if segments.len() >= 3 && matches!(segments[0], "live" | "movie" | "series" | "timeshift") {
        found.extend([segments[1].to_owned(), segments[2].to_owned()]);
    } else if segments.len() == 3 {
        // `http://host/USER/PASS/123`, the short Xtream live form.
        found.extend([segments[0].to_owned(), segments[1].to_owned()]);
    }
    found.retain(|s| s.len() >= 3);
    found.sort_by_key(|s| std::cmp::Reverse(s.len()));
    found
}

/// Makes free text safe to log: any URL-like word becomes `[url]`, any path-like word `[path]`,
/// and anything from `url` that identifies the account `[redacted]`. Control characters go, and
/// the result is at most `max` characters.
pub(crate) fn redact(text: &str, url: Option<&Url>, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max));
    for word in text.split_inclusive(char::is_whitespace) {
        let bare = word.trim_end();
        if bare.contains("://") || bare.contains("url=") {
            out.push_str("[url]");
        } else if bare.matches('/').count() >= 2 {
            out.push_str("[path]");
        } else {
            out.push_str(bare);
        }
        if bare.len() < word.len() {
            out.push(' ');
        }
    }
    if let Some(url) = url {
        for secret in secrets(url) {
            out = out.replace(&secret, "[redacted]");
        }
    }
    out.chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The same stream: equal but for the last segment's extension (the proxy's converter reads a
/// live `.m3u8` channel as its `.ts` form).
fn same_stream(a: &Url, b: &Url) -> bool {
    let stem = |u: &Url| {
        let path = u.path();
        let cut = path
            .rfind('.')
            .filter(|&dot| dot > path.rfind('/').unwrap_or(0));
        (
            u.host_str().map(str::to_owned),
            u.port_or_known_default(),
            path[..cut.unwrap_or(path.len())].to_owned(),
        )
    };
    stem(a) == stem(b)
}

/// `[session] ` for the open session playing `url` (the newest, if several), so a line from the
/// proxy or ffmpeg says whose it is; empty if none is or logging is off.
pub(crate) fn tag(state: &AppState, url: &Url) -> String {
    if state.logs.is_none() {
        return String::new();
    }
    let sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
    sessions
        .iter()
        .filter(|(_, s)| s.expires > Instant::now() && same_stream(&s.url, url))
        .max_by_key(|(_, s)| s.opened)
        .map(|(id, _)| format!("[{}] ", short(id)))
        .unwrap_or_default()
}

/// The first 8 characters of a session id: enough to tell sessions apart in a log.
fn short(id: &str) -> &str {
    &id[..id.len().min(8)]
}

/// The page opens a session for a stream it is about to play. The response contains only an ID.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Response {
    if state.logs.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !from_app(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    if input.url.len() > 4096 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(url) = Url::parse(&input.url) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !matches!(url.scheme(), "http" | "https") || !url_ok(&state.approved, &url) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .filter(|v| !v.is_empty())
        .unwrap_or(crate::FALLBACK_UA)
        .to_owned();
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let id = bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    });
    {
        let mut sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
        clean(&mut sessions);
        // Zapping through channels opens many: the oldest goes, never the one just opened.
        while sessions.len() >= MAX_SESSIONS {
            let oldest = sessions
                .iter()
                .min_by_key(|(_, s)| s.expires)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => sessions.remove(&id),
                None => break,
            };
        }
        sessions.insert(
            id.clone(),
            Session {
                url,
                agent,
                opened: Instant::now(),
                expires: Instant::now() + LIFE,
                probed: None,
            },
        );
    }
    Json(Entry {
        id,
        seconds_left: LIFE.as_secs(),
    })
    .into_response()
}

/// The sessions open now, for a local chatbot to pick one to probe.
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if state.logs.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !from_app(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
    clean(&mut sessions);
    Json(
        sessions
            .iter()
            .map(|(id, session)| Entry {
                id: id.clone(),
                seconds_left: session
                    .expires
                    .saturating_duration_since(Instant::now())
                    .as_secs(),
            })
            .collect::<Vec<_>>(),
    )
    .into_response()
}

/// One event from the page's player, written as `[session] event key=value …`. Keys are
/// restricted to short identifiers; strings are redacted (they may be error messages).
pub async fn event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<serde_json::Map<String, Value>>,
) -> Response {
    if state.logs.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !from_app(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let (url, probe_now) = {
        let mut sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
        clean(&mut sessions);
        let Some(session) = sessions.get_mut(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        session.expires = Instant::now() + LIFE;
        let name = input.get("event").and_then(Value::as_str).unwrap_or("");
        let url = session.url.clone();
        if name == "closed" {
            // The viewer left: its stream is no longer anyone's.
            sessions.remove(&id);
            drop(sessions);
            note(&state, &event_line(&id, &input, &url));
            return StatusCode::NO_CONTENT.into_response();
        }
        let Some(session) = sessions.get_mut(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        let probe_now =
            TROUBLE.contains(&name) && session.probed.is_none_or(|at| at.elapsed() >= PROBE_GAP);
        if probe_now {
            session.probed = Some(Instant::now());
        }
        ((session.url.clone(), session.agent.clone()), probe_now)
    };
    note(&state, &event_line(&id, &input, &url.0));
    if probe_now {
        let state = state.clone();
        tokio::spawn(async move {
            let found = run_probe(&state, url.0, &url.1).await;
            note(&state, &probe_line(&id, &found, "auto"));
        });
    }
    StatusCode::NO_CONTENT.into_response()
}

fn event_line(id: &str, input: &serde_json::Map<String, Value>, url: &Url) -> String {
    let ident = |s: &str| {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(24)
            .collect::<String>()
    };
    let name = ident(input.get("event").and_then(Value::as_str).unwrap_or("?"));
    let mut line = format!("[{}] {name}", short(id));
    for (key, value) in input.iter().filter(|(k, _)| *k != "event").take(24) {
        let value = match value {
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => format!("{:?}", redact(s, Some(url), 200)),
            Value::Null => continue,
            _ => "[object]".into(),
        };
        let _ = write!(line, " {}={value}", ident(key));
    }
    line
}

#[derive(Serialize)]
struct Report {
    /// The one-phrase reading of everything below (see [`verdict`]).
    verdict: &'static str,
    status: &'static str,
    http_status: Option<u16>,
    kind: &'static str,
    sample_bytes: usize,
    /// How long the sample took, and how fast it came: a slow provider shows here.
    ms: u64,
    kbps: u64,
    ts_sync: bool,
    video_codec: Option<&'static str>,
    audio_codec: Option<&'static str>,
    playlist_hops: u8,
    /// From the last media playlist read: its segment length, segments listed, whether it's live.
    target_duration: Option<u32>,
    segments: Option<usize>,
    live: Option<bool>,
}

fn report(status: &'static str, http_status: Option<u16>, bytes: &[u8], hops: u8) -> Report {
    let sniffed = xtream::Sniff::of(bytes);
    let (kind, video_codec, audio_codec) = (sniffed.container, sniffed.video, sniffed.audio);
    let ts_sync = kind == "mpeg_ts";
    Report {
        verdict: "",
        status,
        http_status,
        kind,
        sample_bytes: bytes.len(),
        ms: 0,
        kbps: 0,
        ts_sync,
        video_codec,
        audio_codec,
        playlist_hops: hops,
        target_duration: None,
        segments: None,
        live: None,
    }
}

/// Fetches a small sample of the stream with the proxy's network rules, following up to two
/// playlist links. No URL or provider text comes back.
async fn run_probe(state: &AppState, url: Url, agent: &str) -> Report {
    let task = async {
        let mut current = url;
        let mut media = (None, None, None);
        for hop in 0..=2_u8 {
            let started = Instant::now();
            let mut response = state
                .http
                .get(current.clone())
                .header("range", "bytes=0-65535")
                .header("user-agent", agent)
                .send()
                .await
                .map_err(|e| {
                    if e.is_timeout() {
                        "timed_out"
                    } else {
                        "request_failed"
                    }
                })?;
            let status = response.status().as_u16();
            let final_url = response.url().clone();
            let mut sample = Vec::with_capacity(SAMPLE_LIMIT);
            while sample.len() < SAMPLE_LIMIT {
                let chunk = response.chunk().await.map_err(|_| "read_failed")?;
                let Some(chunk) = chunk else { break };
                sample.extend_from_slice(&chunk[..chunk.len().min(SAMPLE_LIMIT - sample.len())]);
            }
            let ms = started.elapsed().as_millis() as u64;
            if hop < 2 && status < 400 && sample.starts_with(b"#EXTM3U") {
                let text = String::from_utf8_lossy(&sample);
                if text.contains("#EXTINF") {
                    media = (
                        text.lines()
                            .find_map(|l| l.strip_prefix("#EXT-X-TARGETDURATION:"))
                            .and_then(|d| d.trim().parse().ok()),
                        Some(text.matches("#EXTINF").count()),
                        Some(!text.contains("#EXT-X-ENDLIST")),
                    );
                }
                if let Some(path) = xtream::sniff::next_in(&text) {
                    let next = final_url.join(path).map_err(|_| "invalid_playlist_link")?;
                    if !url_ok(&state.approved, &next) {
                        return Err("blocked_playlist_link");
                    }
                    current = next;
                    continue;
                }
            }
            let mut found = report(
                if status < 400 { "ok" } else { "http_error" },
                Some(status),
                &sample,
                hop,
            );
            found.ms = ms;
            found.kbps = (sample.len() as u64 * 8).checked_div(ms).unwrap_or(0);
            (found.target_duration, found.segments, found.live) = media;
            return Ok::<_, &'static str>(found);
        }
        Err("playlist_too_deep")
    };
    let mut found = match tokio::time::timeout(Duration::from_secs(15), task).await {
        Ok(Ok(found)) => found,
        Ok(Err(reason)) => report(reason, None, &[], 0),
        Err(_) => report("timed_out", None, &[], 0),
    };
    found.verdict = verdict(&found);
    found
}

/// What a probe means, in a phrase: who is at fault and what would help.
fn verdict(r: &Report) -> &'static str {
    match (r.status, r.http_status) {
        ("timed_out", _) => "provider_not_answering",
        ("request_failed" | "read_failed", _) => "provider_unreachable",
        ("blocked_playlist_link", _) => "playlist_points_at_a_private_address",
        ("invalid_playlist_link" | "playlist_too_deep", _) => "playlist_unreadable",
        (_, Some(401 | 403)) => "provider_refused_account_or_connection_limit",
        (_, Some(404 | 410)) => "channel_offline_or_removed",
        (_, Some(429 | 458 | 509 | 551)) => "provider_connection_limit",
        (_, Some(500..)) => "provider_server_error",
        (_, Some(400..)) => "provider_refused_request",
        _ if r.kind == "html_or_xml" => "provider_sent_a_web_page_not_video",
        _ if r.kind == "unknown" && r.sample_bytes < 188 => "empty_or_tiny_response",
        // Live segments must arrive faster than they play. The sample is at most 64 KiB, so
        // when even that took longer than a whole segment lasts, the provider can't keep up.
        _ if r.live == Some(true)
            && r.target_duration
                .is_some_and(|d| r.ms > u64::from(d) * 1000) =>
        {
            "provider_too_slow_for_live"
        }
        // 64 KiB is too little to measure speed by (latency dominates); only a crawl counts.
        _ if r.ms > 2000 && r.sample_bytes >= SAMPLE_LIMIT => "provider_slow",
        _ if r.video_codec == Some("hevc") => "ok_hevc_needs_conversion_on_most_browsers",
        _ if matches!(r.audio_codec, Some("ac3" | "eac3" | "mp2")) => {
            "ok_audio_needs_conversion_or_rust_decoding"
        }
        _ => "looks_ok",
    }
}

fn probe_line(id: &str, r: &Report, why: &str) -> String {
    // Only fixed labels and numbers: nothing here came from the provider as text.
    let opt = |v: Option<String>| v.unwrap_or_else(|| "-".into());
    format!(
        "[{}] probe ({why}) verdict={} status={} http={} kind={} bytes={} ms={} kbps={} ts_sync={} video={} audio={} hops={} target_duration={} segments={} live={}",
        short(id),
        r.verdict,
        r.status,
        opt(r.http_status.map(|s| s.to_string())),
        r.kind,
        r.sample_bytes,
        r.ms,
        r.kbps,
        r.ts_sync,
        r.video_codec.unwrap_or("-"),
        r.audio_codec.unwrap_or("-"),
        r.playlist_hops,
        opt(r.target_duration.map(|d| d.to_string())),
        opt(r.segments.map(|n| n.to_string())),
        opt(r.live.map(|l| l.to_string())),
    )
}

/// `POST /diagnostics/{id}/probe`: probe a session's stream now, and log it.
pub async fn probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if state.logs.is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !from_app(&headers) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let url = {
        let mut sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
        clean(&mut sessions);
        let Some(session) = sessions.get(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        (session.url.clone(), session.agent.clone())
    };
    let found = run_probe(&state, url.0, &url.1).await;
    note(&state, &probe_line(&id, &found, "asked"));
    Json(found).into_response()
}

/// What a proxied request was for, from its path, so a log line can say "segment" not a URL.
pub(crate) fn kind_of(url: &Url) -> &'static str {
    let path = url.path().to_ascii_lowercase();
    let ext = path.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "m3u8" | "m3u" => "playlist",
        "ts" | "m4s" | "aac" | "mp3" | "m4a" | "m4v" => "segment",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "file",
        "php" => "api",
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "svg" => "image",
        _ if path.contains("/live/") => "live",
        _ => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_report_never_contains_source_text() {
        let report = serde_json::to_string(&report(
            "ok",
            Some(200),
            b"#EXTM3U\nhttps://user:secret@host",
            0,
        ))
        .unwrap();
        assert!(report.contains("hls_or_m3u"));
        assert!(!report.contains("secret"));
    }

    #[test]
    fn redaction_removes_addresses_and_account_details() {
        let url = Url::parse("http://tv.example.com:8080/live/alice/hunter22/123.ts?token=abcdef")
            .unwrap();
        for text in [
            "fetch failed: http://tv.example.com:8080/live/alice/hunter22/123.ts",
            "segment /live/alice/hunter22/9.ts returned 404",
            "/proxy?url=http%3A%2F%2Ftv.example.com",
            "login for alice with hunter22 refused by tv.example.com",
            "token abcdef expired",
        ] {
            let clean = redact(text, Some(&url), 200);
            for secret in ["alice", "hunter22", "tv.example.com", "abcdef", "http"] {
                assert!(!clean.contains(secret), "{text} -> {clean}");
            }
        }
        // Ordinary words survive, and length is capped.
        assert_eq!(
            redact("decoder said\nno frames", Some(&url), 200),
            "decoder said no frames"
        );
        assert_eq!(redact(&"x".repeat(500), None, 10).len(), 10);
    }

    #[test]
    fn short_xtream_paths_are_secrets_too() {
        let url = Url::parse("http://host.tv/bob/s3cret/42").unwrap();
        assert!(!redact("bob s3cret", Some(&url), 50).contains("bob"));
        assert!(!redact("bob s3cret", Some(&url), 50).contains("s3cret"));
    }

    #[test]
    fn events_become_one_safe_line() {
        let url = Url::parse("http://host.tv/live/bob/pw123/1.m3u8").unwrap();
        let input = serde_json::json!({
            "event": "failure",
            "text": "Playback failed: http://host.tv/live/bob/pw123/1.m3u8 gave 403",
            "ms": 1234,
            "ok": false,
            "bad key!": 1,
            "nested": {"a": 1},
        });
        let line = event_line("0123456789abcdef", input.as_object().unwrap(), &url);
        assert!(line.starts_with("[01234567] failure"), "{line}");
        assert!(line.contains("ms=1234") && line.contains("ok=false"));
        assert!(line.contains("badkey=1") && line.contains("nested=[object]"));
        assert!(!line.contains("pw123") && !line.contains("bob") && !line.contains("host.tv"));
    }

    #[test]
    fn verdicts_name_the_likely_cause() {
        let with = |status, http, bytes: &[u8]| {
            let r = report(status, http, bytes, 0);
            verdict(&r)
        };
        assert_eq!(
            with("http_error", Some(404), b"nope"),
            "channel_offline_or_removed"
        );
        assert_eq!(
            with("http_error", Some(403), b""),
            "provider_refused_account_or_connection_limit"
        );
        assert_eq!(with("timed_out", None, b""), "provider_not_answering");
        assert_eq!(
            with("ok", Some(200), b"<html>login</html>"),
            "provider_sent_a_web_page_not_video"
        );
        let mut slow = report("ok", Some(200), &[0x47; 400], 1);
        (slow.live, slow.target_duration, slow.ms) = (Some(true), Some(4), 9000);
        assert_eq!(verdict(&slow), "provider_too_slow_for_live");
        slow.ms = 300;
        assert_eq!(verdict(&slow), "looks_ok");
    }

    #[test]
    fn a_channel_is_the_same_stream_as_its_ts_form() {
        let m3u8 = Url::parse("http://h.tv/live/u/p/7.m3u8").unwrap();
        assert!(same_stream(
            &m3u8,
            &Url::parse("http://h.tv/live/u/p/7.ts").unwrap()
        ));
        assert!(same_stream(
            &m3u8,
            &Url::parse("http://h.tv/live/u/p/7").unwrap()
        ));
        assert!(!same_stream(
            &m3u8,
            &Url::parse("http://h.tv/live/u/p/8.ts").unwrap()
        ));
        assert!(!same_stream(
            &m3u8,
            &Url::parse("http://other.tv/live/u/p/7.ts").unwrap()
        ));
    }
}
