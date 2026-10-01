//! The standard player's one decision: given what a stream is ([`xtream::Sniff`]) and what this
//! browser decodes, play it as is, or have the proxy's ffmpeg convert its sound or its video. The
//! experimental (rstreamkit) player never comes here: it reads and decodes streams itself.

use std::{future::Future, task::Poll, time::Duration};

use web_sys::{
    js_sys,
    wasm_bindgen::{JsCast, JsValue},
};
pub use xtream::sniff::{Plan, plan};
use xtream::{Client, Sniff};

/// What the stream at `url` is, or `None` if that can't be told within a few seconds (the player
/// then falls back on the proxy's own check).
pub async fn sniff(client: &Client, url: &str) -> Option<Sniff> {
    let media = xtream::Url::parse(url).ok()?;
    let mut work = std::pin::pin!(client.sniff(&media));
    let mut timer = std::pin::pin!(rstreamkit::mse::sleep(Duration::from_secs(5)));
    std::future::poll_fn(|cx| {
        if let Poll::Ready(found) = work.as_mut().poll(cx) {
            return Poll::Ready(found.ok());
        }
        timer.as_mut().poll(cx).map(|()| None)
    })
    .await
}

/// The proxy's conversion for `plan`, started without a check (the sniff already answered it).
pub fn conversion(client: &Client, url: &str, plan: Plan) -> Option<String> {
    let media = xtream::Url::parse(url).ok()?;
    Some(
        client
            .convert_known(&media, plan.video())?
            .at(0)
            .to_string(),
    )
}

/// The plan last found for channel `id` on this profile: a channel opened before starts the right
/// way at once, without a sniff. One small map in the browser's storage, read once per channel.
pub fn remembered(id: u64) -> Option<Plan> {
    plans().get(&id).copied().and_then(Plan::from_name)
}

pub fn remember(id: u64, plan: Plan) {
    let mut all = plans();
    if all.get(&id).copied() != Some(plan.name()) {
        all.insert(id, plan.name());
        // ponytail: whole-map rewrite per change; a few KiB even for hundreds of channels.
        if let (Some(store), Ok(json)) = (crate::storage(), serde_json::to_string(&all)) {
            let _ = store.set_item(&crate::shelves::scoped_key("plans"), &json);
        }
    }
}

fn plans() -> std::collections::HashMap<u64, &'static str> {
    let raw: std::collections::HashMap<u64, String> = crate::storage()
        .and_then(|s| {
            s.get_item(&crate::shelves::scoped_key("plans"))
                .ok()
                .flatten()
        })
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    raw.into_iter()
        .filter_map(|(id, name)| Some((id, Plan::from_name(&name)?.name())))
        .collect()
}

/// Whether this browser's media pipeline decodes `mime` (`MediaSource.isTypeSupported`).
pub fn browser_decodes(mime: &str) -> bool {
    js_sys::Reflect::get(&js_sys::global(), &"MediaSource".into())
        .ok()
        .filter(|m| !m.is_undefined())
        .and_then(|m| {
            let check = js_sys::Reflect::get(&m, &"isTypeSupported".into()).ok()?;
            let check: &js_sys::Function = check.dyn_ref()?;
            check.call1(&m, &JsValue::from_str(mime)).ok()?.as_bool()
        })
        .unwrap_or(false)
}
