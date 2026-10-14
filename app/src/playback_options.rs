//! Channel context actions update preferences without interrupting the active player.
use super::*;

#[derive(Clone, Copy)]
pub(crate) struct Options {
    pub watching: Signal<Option<u64>>,
    pub changed: Signal<u64>,
    pub menu: Signal<Option<(u64, f64, f64)>>,
}

pub(crate) fn open(mut options: Options, target: Option<u64>, event: MouseEvent) -> bool {
    let Some(target) = target else {
        return false;
    };
    if options.watching.peek().is_none_or(|id| id == target) {
        return false;
    }
    event.prevent_default();
    let point = event.client_coordinates();
    options.menu.set(Some((target, point.x, point.y)));
    true
}

#[component]
pub(crate) fn Menu() -> Element {
    let mut options = use_context::<Options>();
    let Some((target, x, y)) = (options.menu)() else {
        return rsx! {};
    };
    let Some(primary) = (options.watching)() else {
        return rsx! {};
    };
    let selected = preferences::load()
        .live_backups
        .contains(&(primary, target));
    let (x, y) = web_sys::window()
        .and_then(|w| {
            Some((
                x.clamp(8.0, (w.inner_width().ok()?.as_f64()? - 256.0).max(8.0)),
                y.clamp(8.0, (w.inner_height().ok()?.as_f64()? - 100.0).max(8.0)),
            ))
        })
        .unwrap_or((8.0, 8.0));
    rsx! {
        div { class: "context-dismiss", onclick: move |_| options.menu.set(None),
            oncontextmenu: move |e| { e.prevent_default(); options.menu.set(None); },
        }
        div { class: "playback-context", role: "menu", aria_label: "Channel options", style: "left:{x}px;top:{y}px",
            onkeydown: move |e| if e.key() == Key::Escape { options.menu.set(None); },
            button { role: "menuitem", autofocus: true,
                onclick: move |_| {
                    preferences::update(|p| preferences::set_backup(p, primary, (!selected).then_some(target)));
                    options.changed += 1;
                    options.menu.set(None);
                },
                if selected { "Remove playback option" } else { "Add as playback option" }
            }
        }
    }
}
