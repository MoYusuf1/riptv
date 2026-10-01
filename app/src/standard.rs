//! The standard player's live path: the proxy's `/live` stream, which reads the channel, works out
//! its codecs and converts only what this browser can't decode (see `proxy/src/live.rs`). The
//! experimental (rstreamkit) player never comes here, and this never uses rstreamkit.

use web_sys::{
    js_sys,
    wasm_bindgen::{JsCast, JsValue},
};
use xtream::Client;

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
