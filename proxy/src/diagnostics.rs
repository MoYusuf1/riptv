//! Opt-in local stream diagnostics. Credentials stay in this process's memory and never appear
//! in a diagnostic response or a subprocess command line.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::{AppState, from_app, url_ok};

const LIFE: Duration = Duration::from_secs(600);
const MAX_SESSIONS: usize = 8;
const SAMPLE_LIMIT: usize = 64 * 1024;

#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<HashMap<String, Session>>>);

#[derive(Clone)]
struct Session {
    url: Url,
    expires: Instant,
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

fn denied() -> Response {
    StatusCode::FORBIDDEN.into_response()
}

fn clean(sessions: &mut HashMap<String, Session>) {
    let now = Instant::now();
    sessions.retain(|_, session| session.expires > now);
}

/// The page explicitly shares one stream for ten minutes. The response contains only an ID.
pub async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> Response {
    if !from_app(&headers) {
        return denied();
    }
    if input.url.len() > 4096 {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let Ok(url) = Url::parse(&input.url) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !matches!(url.scheme(), "http" | "https") || !url_ok(&state.approved, &url) {
        return denied();
    }
    let mut bytes = [0_u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let id = bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let mut sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
    clean(&mut sessions);
    if sessions.len() >= MAX_SESSIONS {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    sessions.insert(
        id.clone(),
        Session {
            url,
            expires: Instant::now() + LIFE,
        },
    );
    Json(Entry {
        id,
        seconds_left: LIFE.as_secs(),
    })
    .into_response()
}

/// Allows both local chatbots to discover active sessions without browser-storage access.
pub async fn list(State(state): State<AppState>, headers: HeaderMap) -> Response {
    if !from_app(&headers) {
        return denied();
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

#[derive(Serialize)]
struct Report {
    status: &'static str,
    http_status: Option<u16>,
    kind: &'static str,
    sample_bytes: usize,
    ts_sync: bool,
    video_codec: Option<&'static str>,
    audio_codec: Option<&'static str>,
    playlist_hops: u8,
}

fn tracks(bytes: &[u8]) -> (Option<&'static str>, Option<&'static str>) {
    let mut found = (None, None);
    for packet in bytes.as_chunks::<188>().0 {
        if packet[0] != 0x47 || packet[1] & 0x40 == 0 {
            continue;
        }
        let mut offset = 4;
        if packet[3] & 0x20 != 0 {
            offset += 1 + usize::from(packet[4]);
        }
        if packet[3] & 0x10 == 0 || offset >= 188 {
            continue;
        }
        offset += 1 + usize::from(packet[offset]); // payload-unit-start pointer
        if offset + 12 > 188 || packet[offset] != 0x02 {
            continue;
        }
        let length =
            (usize::from(packet[offset + 1] & 0x0f) << 8) | usize::from(packet[offset + 2]);
        let end = (offset + 3 + length).saturating_sub(4).min(188);
        let info =
            (usize::from(packet[offset + 10] & 0x0f) << 8) | usize::from(packet[offset + 11]);
        offset += 12 + info;
        while offset + 5 <= end {
            match packet[offset] {
                0x1b => found.0 = Some("h264"),
                0x24 => found.0 = Some("hevc"),
                0x02 => found.0 = Some("mpeg2video"),
                0x0f | 0x11 => found.1 = Some("aac"),
                0x03 | 0x04 => found.1 = Some("mp2"),
                0x81 => found.1 = Some("ac3"),
                _ => {}
            }
            let es_info =
                (usize::from(packet[offset + 3] & 0x0f) << 8) | usize::from(packet[offset + 4]);
            offset += 5 + es_info;
        }
        if found.0.is_some() || found.1.is_some() {
            break;
        }
    }
    found
}

fn report(
    status: &'static str,
    http_status: Option<u16>,
    bytes: &[u8],
    playlist_hops: u8,
) -> Report {
    let kind = if bytes.starts_with(b"#EXTM3U") {
        "hls_or_m3u"
    } else if bytes.len() > 8 && &bytes[4..8] == b"ftyp" {
        "mp4"
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        "matroska"
    } else if bytes.first() == Some(&0x47) {
        "mpeg_ts"
    } else {
        "unknown"
    };
    let ts_sync = bytes.len() > 376 && bytes[0] == 0x47 && bytes[188] == 0x47 && bytes[376] == 0x47;
    let (video_codec, audio_codec) = if ts_sync { tracks(bytes) } else { (None, None) };
    Report {
        status,
        http_status,
        kind,
        sample_bytes: bytes.len(),
        ts_sync,
        video_codec,
        audio_codec,
        playlist_hops,
    }
}

/// Fetches a small sample with the proxy's network restrictions. No URL or provider text returns.
pub async fn probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if !from_app(&headers) {
        return denied();
    }
    let url = {
        let mut sessions = state.diagnostics.0.lock().expect("diagnostic sessions");
        clean(&mut sessions);
        let Some(session) = sessions.get(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        session.url.clone()
    };
    let task = async {
        let mut current = url;
        for hop in 0..=2_u8 {
            let mut response = state
                .http
                .get(current.clone())
                .header("range", "bytes=0-65535")
                .send()
                .await
                .map_err(|_| "request_failed")?;
            let status = response.status().as_u16();
            let final_url = response.url().clone();
            let mut sample = Vec::with_capacity(SAMPLE_LIMIT);
            while sample.len() < SAMPLE_LIMIT {
                let chunk = response.chunk().await.map_err(|_| "read_failed")?;
                let Some(chunk) = chunk else { break };
                sample.extend_from_slice(&chunk[..chunk.len().min(SAMPLE_LIMIT - sample.len())]);
            }
            if hop < 2 && sample.starts_with(b"#EXTM3U") {
                let text = String::from_utf8_lossy(&sample);
                if let Some(path) = text
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty() && !line.starts_with('#'))
                {
                    let next = final_url.join(path).map_err(|_| "invalid_playlist_link")?;
                    if !url_ok(&state.approved, &next) {
                        return Err("blocked_playlist_link");
                    }
                    current = next;
                    continue;
                }
            }
            return Ok::<_, &'static str>(report("ok", Some(status), &sample, hop));
        }
        Err("playlist_too_deep")
    };
    let found = match tokio::time::timeout(Duration::from_secs(15), task).await {
        Ok(Ok(found)) => found,
        Ok(Err(reason)) => report(reason, None, &[], 0),
        Err(_) => report("timed_out", None, &[], 0),
    };
    if let Some(path) = &state.logs {
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            // The report has only fixed labels, numeric status/size, and no provider text.
            let _ = writeln!(
                file,
                "{} session={} status={} http={:?} kind={} sample_bytes={} ts_sync={} video={:?} audio={:?} playlist_hops={}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
                id,
                found.status,
                found.http_status,
                found.kind,
                found.sample_bytes,
                found.ts_sync,
                found.video_codec,
                found.audio_codec,
                found.playlist_hops
            );
        }
    }
    Json(found).into_response()
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
}
