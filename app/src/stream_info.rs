//! A compact, readable snapshot. Sampling belongs to the player and stops when this is closed.
use super::*;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Readout {
    pub picture: Option<(u32, u32)>,
    pub fps: Option<u32>,
    pub dropped: u32,
    pub buffer_seconds: u32,
    pub playback: &'static str,
}

impl Default for Readout {
    fn default() -> Self {
        Self {
            picture: None,
            fps: None,
            dropped: 0,
            buffer_seconds: 0,
            playback: "Preparing",
        }
    }
}

#[component]
pub(crate) fn StreamInfo(readout: Readout, onclose: EventHandler<()>) -> Element {
    let picture = readout
        .picture
        .map_or_else(|| "Not ready".into(), |(w, h)| format!("{w} × {h}"));
    let fps = readout
        .fps
        .map_or_else(|| "—".into(), |n| format!("{n} fps"));
    let dropped = thousands(readout.dropped as usize);
    rsx! {
        section { class: "stream-info", id: "stream-info", aria_label: "Stream info",
            div { class: "settings-heading",
                h3 { "Stream info" }
                button { class: "icon-btn", aria_label: "Close stream info", onclick: move |_| onclose.call(()), Icon { d: CLOSE } }
            }
            dl {
                div { dt { "Picture" } dd { "{picture}" } }
                div { dt { "Frame rate" } dd { "{fps}" } }
                div { dt { "Dropped frames" } dd { "{dropped}" } }
                div { dt { "Buffer ahead" } dd { "{readout.buffer_seconds} s" } }
                div { dt { "Playback" } dd { "{readout.playback}" } }
            }
        }
    }
}
