//! One app-wide release check; playback and profile credentials stay out of it.
use super::*;
use wasm_bindgen_futures::JsFuture;
use xtream::updates::UpdateInfo;

pub(crate) const CSS: &str = r#"
.update-notice{position:fixed;z-index:90;right:1rem;bottom:4.5rem;display:flex;align-items:center;gap:.8rem;padding:.8rem 1rem;border:1px solid var(--hair);border-radius:14px;background:#21161c;box-shadow:0 8px 30px #0006;font-size:.85rem}.update-notice a{color:var(--accent)}.update-notice button{color:var(--dim)}.update-status{padding:.5rem .8rem;color:var(--dim);font-size:.75rem}.menu .update-download{display:block;padding:.7rem .8rem;color:var(--accent)}
"#;

async fn fetch(manual: bool) -> Option<UpdateInfo> {
    let init = web_sys::RequestInit::new();
    init.set_method(if manual { "POST" } else { "GET" });
    let request = web_sys::Request::new_with_str_and_init("/updates", &init).ok()?;
    let response: web_sys::Response =
        JsFuture::from(web_sys::window()?.fetch_with_request(&request))
            .await
            .ok()?
            .dyn_into()
            .ok()?;
    if !response.ok() {
        return None;
    }
    let text = JsFuture::from(response.text().ok()?)
        .await
        .ok()?
        .as_string()?;
    serde_json::from_str(&text).ok()
}

pub(crate) fn use_updates() {
    let mut status = use_context_provider(|| Signal::new(None::<UpdateInfo>));
    use_future(move || async move {
        loop {
            if let Some(info) = fetch(false).await {
                status.set(Some(info));
            }
            rstreamkit::mse::sleep(Duration::from_secs(6 * 3600)).await;
        }
    });
}

#[component]
pub(crate) fn Check() -> Element {
    let mut status = use_context::<Signal<Option<UpdateInfo>>>();
    let mut busy = use_signal(|| false);
    let mut message = use_signal(String::new);
    let info = status();
    rsx! {
        button { disabled: busy(), onclick: move |_| {
            busy.set(true);
            spawn(async move {
                let result = fetch(true).await;
                message.set(match result.as_ref() {
                    Some(i) if i.available => "Update available",
                    Some(i) if i.checked => "You’re up to date",
                    _ => "Couldn’t check. Try again.",
                }.into());
                if let Some(i) = result { status.set(Some(i)); }
                busy.set(false);
            });
        }, if busy() { "Checking…" } else { "Check for updates" } }
        if let Some(i) = info {
            p { class: "update-status", "Version {i.current}" }
            if let Some(url) = i.download {
                a { class: "update-download", href: url, target: "_blank", rel: "noopener noreferrer", "Download update" }
            }
        }
        if !message().is_empty() { p { class: "update-status", "{message}" } }
    }
}

#[component]
pub(crate) fn Notice() -> Element {
    let status = use_context::<Signal<Option<UpdateInfo>>>();
    let mut dismissed =
        use_signal(|| storage().and_then(|s| s.get_item("riptv.update-dismissed").ok().flatten()));
    let Some(info) = status().filter(|i| i.available) else {
        return rsx! {};
    };
    let (Some(latest), Some(url)) = (info.latest, info.download) else {
        return rsx! {};
    };
    if dismissed().as_ref() == Some(&latest) {
        return rsx! {};
    }
    rsx! {
        aside { class: "update-notice", aria_label: "App update",
            span { "RIPTV {latest} is ready" }
            a { href: url, target: "_blank", rel: "noopener noreferrer", "Download" }
            button { onclick: move |_| {
                if let Some(s) = storage() { let _ = s.set_item("riptv.update-dismissed", &latest); }
                dismissed.set(Some(latest.clone()));
            }, "Later" }
        }
    }
}
