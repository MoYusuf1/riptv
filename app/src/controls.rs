//! Small player-control primitives shared by live TV and on-demand playback.

use dioxus::prelude::*;
use std::{cell::Cell, rc::Rc, time::Duration};
use web_sys::js_sys;
use web_sys::wasm_bindgen::{JsCast, closure::Closure};

/// A scoped outside-click listener: removed when the dropdown closes or its player unmounts.
pub(crate) struct OutsideClick {
    document: web_sys::Document,
    callback: Closure<dyn FnMut(web_sys::Event)>,
}
impl OutsideClick {
    pub(crate) fn new(inside: &'static str, mut close: impl FnMut() + 'static) -> Option<Self> {
        let document = web_sys::window()?.document()?;
        let callback = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let internal = event
                .target()
                .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                .is_some_and(|el| el.closest(inside).ok().flatten().is_some());
            if !internal {
                close();
            }
        });
        document
            .add_event_listener_with_callback("click", callback.as_ref().unchecked_ref())
            .ok()?;
        Some(Self { document, callback })
    }
}
impl Drop for OutsideClick {
    fn drop(&mut self) {
        let _ = self
            .document
            .remove_event_listener_with_callback("click", self.callback.as_ref().unchecked_ref());
    }
}

/// Shows a player's controls on activity and hides them after `delay_ms` without any. At most one
/// timer runs, and none while the controls are hidden or nothing is happening, so an idle player
/// costs no wake-ups at all (polling would cost a few per second, for as long as it plays).
#[derive(Clone)]
pub struct IdleHide {
    last: Rc<Cell<f64>>,
    waiting: Rc<Cell<bool>>,
    active: Signal<bool>,
    delay_ms: f64,
}

impl IdleHide {
    /// The controls start shown, and hide `delay_ms` from now unless something wakes them.
    pub fn new(active: Signal<bool>, delay_ms: f64) -> Self {
        let idle = IdleHide {
            last: Rc::new(Cell::new(0.0)),
            waiting: Rc::new(Cell::new(false)),
            active,
            delay_ms,
        };
        idle.wake();
        idle
    }

    /// Something happened (a pointer move, a key). Safe to call from anywhere, a raw browser
    /// listener included: it sets a signal and starts a plain browser task, nothing of Dioxus's.
    pub fn wake(&self) {
        self.last.set(js_sys::Date::now());
        let mut active = self.active;
        // (Read into a local first: the guard must be gone before the write.)
        let hidden = active.try_peek().map(|a| !*a).unwrap_or(false);
        if hidden {
            active.set(true);
        }
        if !self.waiting.replace(true) {
            let (last, waiting, delay) = (self.last.clone(), self.waiting.clone(), self.delay_ms);
            wasm_bindgen_futures::spawn_local(async move {
                // Sleep until the deadline the latest activity set; activity meanwhile only moves it.
                loop {
                    let left = delay - (js_sys::Date::now() - last.get());
                    if left <= 0.0 {
                        break;
                    }
                    rstreamkit::mse::sleep(Duration::from_millis(left.ceil() as u64)).await;
                }
                waiting.set(false);
                // The player may be gone by now, and its signal with it.
                if active.try_peek().is_ok() {
                    active.set(false);
                }
            });
        }
    }
}

/// Centered skip icon with a legible interval at small control sizes.
#[component]
pub fn Skip(back: bool) -> Element {
    rsx! {
        svg {
            view_box: "0 0 24 24",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "1.8",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            path { d: if back { "M4 9.5a8 8 0 1 1 .7 6.1M4 5v4.5h4.5" } else { "M20 9.5a8 8 0 1 0-.7 6.1M20 5v4.5h-4.5" } }
            text { x: "12", y: "12", text_anchor: "middle", dominant_baseline: "central", font_size: "7.4", font_weight: "700", fill: "currentColor", stroke: "none", "15" }
        }
    }
}

/// A player starting: a ring that always turns (so waiting never looks stuck) and fills as the
/// player reaches each stage: 0 asked, 1 the stream answered, 2 its format is known, 3 a picture is
/// decoded, 4 playing. Between stages it eases toward the next mark. With `icon` (a channel logo),
/// the logo sits inside. All motion is CSS (compositor-only transforms): no timers, no re-renders.
#[component]
pub fn LoadRing(stage: u8, icon: Option<String>, percent: Option<u32>) -> Element {
    // Circumference of r = 22.
    const LENGTH: f64 = 138.23;
    let filled = [0.12, 0.45, 0.68, 0.88, 1.0][usize::from(stage.min(4))];
    let offset = LENGTH * (1.0 - filled);
    let logo = icon.filter(|src| src.starts_with("http"));
    // With a percentage, the ring and the number are both drawn from one animated `--p`, so
    // they count up together, smoothly, with no script running per frame.
    let (class, style, now) = match percent {
        Some(p) => ("loader pct", format!("--p:{}", p.min(100)), p.min(100)),
        None => ("loader", String::new(), (filled * 100.0) as u32),
    };
    rsx! {
        div { class, style, role: "progressbar", "aria-label": "Loading", "aria-valuenow": "{now}",
            svg { view_box: "0 0 50 50",
                defs {
                    linearGradient { id: "ring-grad", x1: "0", y1: "0", x2: "1", y2: "1",
                        stop { offset: "0", stop_color: "#fff" }
                        stop { offset: "1", stop_color: "var(--accent)" }
                    }
                }
                circle { class: "ring-track", cx: "25", cy: "25", r: "22" }
                if percent.is_some() {
                    circle { class: "ring-fill", cx: "25", cy: "25", r: "22" }
                } else {
                    circle { class: "ring-fill", cx: "25", cy: "25", r: "22", style: "stroke-dashoffset:{offset:.1}" }
                }
            }
            if percent.is_some() {
                span { class: "loader-pct", small { "%" } }
            } else if let Some(src) = logo {
                img { class: "loader-logo", src: "{src}", alt: "", decoding: "async" }
            } else {
                i { class: "loader-dot" }
            }
        }
    }
}
