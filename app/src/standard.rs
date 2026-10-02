//! The standard player's live path: the proxy's `/live` stream, which reads the channel, works out
//! its codecs and converts only what this browser can't decode (see `proxy/src/live.rs`). The
//! experimental (rstreamkit) player never comes here, and this never uses rstreamkit.

use std::time::Duration;
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    AbortController, HtmlVideoElement, MediaSource, MediaSourceReadyState, RequestInit, Response,
    SourceBuffer,
};
use web_sys::{
    js_sys,
    wasm_bindgen::{JsCast, JsValue},
};
use xtream::Client;

/// Owns the one live fetch. Dropping it cancels the request and releases its object URL.
pub struct Stream {
    abort: AbortController,
    object: String,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.abort.abort();
        let _ = web_sys::Url::revoke_object_url(&self.object);
    }
}

pub fn start(
    video: HtmlVideoElement,
    src: String,
    mut on_error: impl FnMut(String) + 'static,
) -> Result<Stream, String> {
    let media = MediaSource::new().map_err(js_error)?;
    let object = web_sys::Url::create_object_url_with_source(&media).map_err(js_error)?;
    let abort = AbortController::new().map_err(js_error)?;
    let signal = abort.signal();
    video.set_src(&object);
    video.load();
    spawn_local(async move {
        let result = pump(&video, &media, &src, &signal).await;
        if !signal.aborted()
            && let Err(why) = result
        {
            on_error(why);
        }
    });
    Ok(Stream { abort, object })
}

fn js_error(value: JsValue) -> String {
    value
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&value, &"message".into())
                .ok()
                .and_then(|message| message.as_string())
        })
        .unwrap_or_else(|| "stream operation failed".into())
}

fn browser_error(value: JsValue) -> String {
    format!("browser: {}", js_error(value))
}

async fn idle(buffer: &SourceBuffer, signal: &web_sys::AbortSignal) -> Result<(), String> {
    while buffer.updating() {
        if signal.aborted() {
            return Err("cancelled".into());
        }
        rstreamkit::mse::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

// The actual AVC configuration in the initialization segment, rather than guessing its profile.
fn mime(init: &[u8], video: &str) -> String {
    if video == "none" {
        return "audio/mp4; codecs=\"mp4a.40.2\"".into();
    }
    let codec = if video == "hevc" {
        init.windows(4)
            .position(|bytes| bytes == b"hvcC")
            .and_then(|at| init.get(at + 4..at + 17))
            .map(|config| {
                let profile = config[1];
                let space = ["", "A", "B", "C"][(profile >> 6) as usize];
                let compatibility =
                    u32::from_be_bytes(config[2..6].try_into().unwrap()).reverse_bits();
                let tier = if profile & 0x20 != 0 { 'H' } else { 'L' };
                let mut constraints = config[6..12].to_vec();
                while constraints.last() == Some(&0) {
                    constraints.pop();
                }
                let suffix = constraints
                    .iter()
                    .map(|byte| format!(".{byte:02X}"))
                    .collect::<String>();
                format!(
                    "hvc1.{space}{}.{compatibility:X}.{tier}{}{suffix}",
                    profile & 0x1f,
                    config[12]
                )
            })
            .unwrap_or_else(|| "hvc1.1.6.L120.90".into())
    } else {
        init.windows(4)
            .position(|bytes| bytes == b"avcC")
            .and_then(|at| init.get(at + 5..at + 8))
            .map(|avc| format!("avc1.{:02X}{:02X}{:02X}", avc[0], avc[1], avc[2]))
            .unwrap_or_else(|| "avc1.640028".into())
    };
    format!("video/mp4; codecs=\"{codec},mp4a.40.2\"")
}

fn init_complete(bytes: &[u8]) -> bool {
    let mut at = 0;
    while let Some(header) = bytes.get(at..at + 8) {
        let size = u32::from_be_bytes(header[..4].try_into().unwrap()) as usize;
        if size < 8 || size > bytes.len().saturating_sub(at) {
            return false;
        }
        if &header[4..8] == b"moov" {
            return true;
        }
        at += size;
    }
    false
}

async fn pump(
    video: &HtmlVideoElement,
    media: &MediaSource,
    src: &str,
    signal: &web_sys::AbortSignal,
) -> Result<(), String> {
    let options = RequestInit::new();
    options.set_signal(Some(signal));
    let window = web_sys::window().ok_or("no browser window")?;
    let response: Response = JsFuture::from(window.fetch_with_str_and_init(src, &options))
        .await
        .map_err(js_error)?
        .dyn_into()
        .map_err(js_error)?;
    if !response.ok() {
        return Err(response
            .headers()
            .get("x-riptv-error")
            .ok()
            .flatten()
            .unwrap_or_else(|| format!("Stream request failed ({})", response.status())));
    }
    let codec = response
        .headers()
        .get("x-riptv-video")
        .ok()
        .flatten()
        .unwrap_or_else(|| "h264".into());
    let reader: web_sys::ReadableStreamDefaultReader = response
        .body()
        .ok_or("empty stream")?
        .get_reader()
        .dyn_into()
        .map_err(|value| js_error(value.into()))?;
    let mut init = Vec::new();
    let mut buffer = None::<SourceBuffer>;
    loop {
        if signal.aborted() {
            return Ok(());
        }
        // Bound memory while a viewer pauses; still retain a useful live reserve.
        if let Some(sb) = buffer.as_ref() {
            let ranges = sb.buffered().map_err(js_error)?;
            if ranges.length() > 0
                && ranges.end(ranges.length() - 1).unwrap_or(0.0) - video.current_time() > 30.0
            {
                rstreamkit::mse::sleep(Duration::from_millis(100)).await;
                continue;
            }
        }
        let result = JsFuture::from(reader.read()).await.map_err(js_error)?;
        if js_sys::Reflect::get(&result, &"done".into())
            .map_err(js_error)?
            .as_bool()
            == Some(true)
        {
            if let Some(sb) = buffer.as_ref() {
                idle(sb, signal).await?;
            }
            if media.ready_state() == MediaSourceReadyState::Open {
                let _ = media.end_of_stream();
            }
            return Ok(());
        }
        let bytes = js_sys::Uint8Array::new(
            &js_sys::Reflect::get(&result, &"value".into()).map_err(js_error)?,
        );
        let data = if buffer.is_none() {
            init.extend(bytes.to_vec());
            if init.len() > 1024 * 1024 {
                return Err("invalid stream initialization".into());
            }
            if !init_complete(&init) {
                continue;
            }
            for _ in 0..200 {
                if signal.aborted() {
                    return Ok(());
                }
                if media.ready_state() == MediaSourceReadyState::Open {
                    break;
                }
                rstreamkit::mse::sleep(Duration::from_millis(25)).await;
            }
            let sb = media
                .add_source_buffer(&mime(&init, &codec))
                .map_err(browser_error)?;
            buffer = Some(sb);
            js_sys::Uint8Array::from(init.as_slice())
        } else {
            bytes
        };
        let sb = buffer.as_ref().unwrap();
        idle(sb, signal).await?;
        let behind = video.current_time() - 20.0;
        if behind > 0.0 {
            let ranges = sb.buffered().map_err(js_error)?;
            if ranges.length() > 0 && ranges.start(0).unwrap_or(behind) < behind - 5.0 {
                sb.remove(0.0, behind).map_err(js_error)?;
                idle(sb, signal).await?;
            }
        }
        loop {
            match sb.append_buffer_with_array_buffer(&data.buffer()) {
                Ok(()) => break,
                Err(error) => {
                    let quota = js_sys::Reflect::get(&error, &"name".into())
                        .ok()
                        .and_then(|name| name.as_string())
                        .is_some_and(|name| name == "QuotaExceededError");
                    if !quota {
                        return Err(browser_error(error));
                    }
                    if signal.aborted() {
                        return Ok(());
                    }
                    // High-bitrate streams can hit the byte quota before our time limit.
                    // Release old frames, or wait for playback to free space while paused.
                    let old = video.current_time() - 5.0;
                    if old > 0.0 {
                        sb.remove(0.0, old).map_err(js_error)?;
                        idle(sb, signal).await?;
                    }
                    rstreamkit::mse::sleep(Duration::from_millis(250)).await;
                }
            }
        }
        idle(sb, signal).await?;
        init.clear();
    }
}

/// The live stream for `url`: as is where the browser can, or with its video re-encoded.
pub fn live(client: &Client, url: &str, transcode: bool) -> Option<String> {
    let media = xtream::Url::parse(url).ok()?;
    Some(client.live(&media, &can(), transcode)?.to_string())
}

/// The codecs this browser decodes beyond H.264 and AAC, as `/live` takes them.
fn can() -> String {
    [
        ("hevc", r#"video/mp4; codecs="hvc1.1.6.L120.90""#),
        ("ac3", r#"audio/mp4; codecs="ac-3""#),
        ("eac3", r#"audio/mp4; codecs="ec-3""#),
    ]
    .into_iter()
    .filter(|(_, mime)| decodes(mime))
    .map(|(name, _)| name)
    .collect::<Vec<_>>()
    .join(",")
}

/// Whether the page can play a fragmented-MP4 stream: Media Source is the tell. An iPhone has
/// none, and plays live channels through its own HLS player instead.
pub fn plays_live_streams() -> bool {
    media_source().is_some()
}

fn media_source() -> Option<JsValue> {
    js_sys::Reflect::get(&js_sys::global(), &"MediaSource".into())
        .ok()
        .filter(|m| !m.is_undefined())
}

/// Whether this browser's media pipeline decodes `mime` (`MediaSource.isTypeSupported`).
fn decodes(mime: &str) -> bool {
    media_source()
        .and_then(|m| {
            let check = js_sys::Reflect::get(&m, &"isTypeSupported".into()).ok()?;
            let check: &js_sys::Function = check.dyn_ref()?;
            check.call1(&m, &JsValue::from_str(mime)).ok()?.as_bool()
        })
        .unwrap_or(false)
}
