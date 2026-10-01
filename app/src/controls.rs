//! Small player-control primitives shared by live TV and on-demand playback.

use dioxus::prelude::*;
use std::{cell::Cell, rc::Rc, time::Duration};
use web_sys::js_sys;

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
pub fn LoadRing(stage: u8, icon: Option<String>) -> Element {
    // Circumference of r = 22.
    const LENGTH: f64 = 138.23;
    let filled = [0.12, 0.45, 0.68, 0.88, 1.0][usize::from(stage.min(4))];
    let offset = LENGTH * (1.0 - filled);
    let logo = icon.filter(|src| src.starts_with("http"));
    rsx! {
        div { class: "loader", role: "progressbar", "aria-label": "Loading", "aria-valuenow": "{(filled * 100.0) as u32}",
            svg { view_box: "0 0 50 50",
                defs {
                    linearGradient { id: "ring-grad", x1: "0", y1: "0", x2: "1", y2: "1",
                        stop { offset: "0", stop_color: "#fff" }
                        stop { offset: "1", stop_color: "var(--accent)" }
                    }
                }
                circle { class: "ring-track", cx: "25", cy: "25", r: "22" }
                circle { class: "ring-fill", cx: "25", cy: "25", r: "22", style: "stroke-dashoffset:{offset:.1}" }
            }
            if let Some(src) = logo {
                img { class: "loader-logo", src: "{src}", alt: "", decoding: "async" }
            } else {
                i { class: "loader-dot" }
            }
        }
    }
}
