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
pub(crate) fn StreamInfo(readout: Readout) -> Element {
    let picture = readout
        .picture
        .map_or_else(|| "…".into(), |(w, h)| format!("{w}×{h}"));
    let fps = readout
        .fps
        .map_or_else(|| "… fps".into(), |n| format!("{n} fps"));
    rsx! {
        div { class: "stats", id: "stream-info", aria_label: "Stream info",
            "{picture} · {fps} · {readout.dropped} dropped · {readout.buffer_seconds}s buffered · {readout.playback}"
        }
    }
}
