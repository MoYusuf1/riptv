//! Quitting the app: the server stops (it owns the window's lifetime in a download), and the page
//! closes itself where the browser allows it, or says it has quit.
use super::*;
use wasm_bindgen_futures::JsFuture;

pub(crate) const CSS: &str = r#"
.quit-screen{position:fixed;inset:0;display:grid;place-items:center;background:var(--bg);color:var(--dim);font-size:.95rem;animation:fade .4s var(--ease)}
"#;

/// Whether the app has quit; the root shows the quiet closed screen when it has.
pub(crate) fn use_quit_state() -> Signal<bool> {
    use_context_provider(|| Signal::new(false))
}

async fn stop_server() -> bool {
    let init = web_sys::RequestInit::new();
    init.set_method("POST");
    let Ok(request) = web_sys::Request::new_with_str_and_init("/quit", &init) else {
        return false;
    };
    let Some(window) = web_sys::window() else {
        return false;
    };
    match JsFuture::from(window.fetch_with_request(&request)).await {
        Ok(response) => response
            .dyn_into::<web_sys::Response>()
            .is_ok_and(|r| r.ok()),
        Err(_) => false,
    }
}

/// The menu's Quit.
#[component]
pub(crate) fn Quit() -> Element {
    let mut quit = use_context::<Signal<bool>>();
    rsx! {
        button {
            onclick: move |_| {
                spawn(async move {
                    if stop_server().await {
                        // Stop playback first, then close the window where that's allowed.
                        quit.set(true);
                        if let Some(window) = web_sys::window() {
                            let _ = window.close();
                        }
                    }
                });
            },
            "Quit"
        }
    }
}

#[component]
pub(crate) fn Closed() -> Element {
    rsx! { div { class: "quit-screen", "RIPTV has quit" } }
}
