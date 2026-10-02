//! On-demand programme previews. Hovering never opens a playback connection.
use super::*;

pub(crate) struct Preview {
    pub pointer: Signal<bool>,
    pub focused: Signal<bool>,
    pub escaped: Signal<bool>,
    pub content: Element,
}

pub(crate) fn use_preview(id: Option<u64>, anchor: String) -> Preview {
    let session = use_context::<Signal<Option<Client>>>();
    let client = use_hook(|| session.read().clone().expect("logged in"));
    let pointer = use_signal(|| false);
    let focused = use_signal(|| false);
    let escaped = use_signal(|| false);
    let mut tick = use_signal(|| 0_u32);
    // A per-channel, per-profile cache, discarded when the library is unmounted.
    let cache = use_hook(|| Rc::new(RefCell::new(None::<(u64, Vec<xtream::EpgListing>)>)));
    use_future(move || async move {
        loop {
            rstreamkit::mse::sleep(Duration::from_secs(30)).await;
            if pointer() || focused() {
                tick += 1;
            }
        }
    });
    let table = use_resource(move || {
        let visible = (pointer() || focused()) && !escaped();
        let _ = tick();
        let c = client.clone();
        let cache = cache.clone();
        async move {
            let id = id.filter(|_| visible && !c.is_playlist())?;
            // Ignore incidental mouse passes and cancel when the row loses hover/focus.
            rstreamkit::mse::sleep(Duration::from_millis(220)).await;
            let now = now_secs();
            if let Some((at, entries)) = &*cache.borrow()
                && now.saturating_sub(*at) < 60
            {
                return Some(Ok(entries.clone()));
            }
            let result = match c.short_epg(id, 4).await {
                Ok(entries) if xtream::guide::current(&entries, now).is_some() => Ok(entries),
                _ => c.epg_table(id).await,
            };
            if let Ok(entries) = &result {
                *cache.borrow_mut() = Some((now, entries.clone()));
            }
            Some(result)
        }
    });
    let visible = id.is_some() && (pointer() || focused()) && !escaped();
    let content = if visible {
        let position = web_sys::window()
            .and_then(|w| {
                let rect = w
                    .document()?
                    .get_element_by_id(&anchor)?
                    .get_bounding_client_rect();
                let width = w.inner_width().ok()?.as_f64()?;
                let height = w.inner_height().ok()?.as_f64()?;
                let tooltip_width = 320_f64.min((width - 24.0).max(0.0));
                let x = if rect.right() + tooltip_width + 20.0 < width {
                    rect.right() + 10.0
                } else {
                    (rect.left() - tooltip_width - 10.0).max(12.0)
                };
                Some((x, rect.top().clamp(12.0, (height - 240.0).max(12.0))))
            })
            .unwrap_or((12.0, 80.0));
        let data = table.read();
        let listing = data
            .as_ref()
            .and_then(|r| r.as_ref())
            .and_then(|r| r.as_ref().ok())
            .and_then(|entries| xtream::guide::current(entries, now_secs()));
        rsx! {
            span {
                id: "{anchor}-guide", class: "channel-preview", role: "tooltip",
                style: "left:{position.0}px;top:{position.1}px",
                span { class: "preview-label", "ON NOW · PROVIDER GUIDE" }
                if let Some(slot) = listing {
                    strong { if slot.listing.title.trim().is_empty() { "Untitled programme" } else { "{slot.listing.title}" } }
                    time { "{clock(slot.start)} – {clock(slot.end)}" }
                    if !slot.listing.description.trim().is_empty() {
                        span { class: "preview-description", "{slot.listing.description}" }
                    }
                } else if data.as_ref().is_none_or(|result| result.is_none()) {
                    span { class: "dim", "Checking what’s on…" }
                } else {
                    span { class: "dim", "Current programme unavailable" }
                }
            }
        }
    } else {
        rsx! {}
    };
    Preview {
        pointer,
        focused,
        escaped,
        content,
    }
}
