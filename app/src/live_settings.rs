//! Compact live settings. Playback options are added from channel context menus.
use super::*;

#[component]
pub(crate) fn LiveSettings(
    catalog: Snapshot<Library>,
    smoother: bool,
    backup: Option<u64>,
    onmode: EventHandler<bool>,
    onbackup: EventHandler<Option<u64>>,
) -> Element {
    let Items::Live(channels) = &catalog.items else {
        return rsx! {};
    };
    let current = backup.and_then(|id| channels.iter().find(|c| c.stream_id == id));
    rsx! {
            section { class: "live-settings", id: "live-settings", aria_label: "Live playback settings", onclick: move |e| e.stop_propagation(),
                div { class: "sheet-body",
                    div { class: "settings-section",
                        h3 { "Playback" }
                        div { class: "playback-modes", role: "group", aria_label: "Playback mode",
                            button { class: if !smoother { "mode selected" } else { "mode" }, aria_pressed: !smoother, aria_label: "Live: shorter playback delay", onclick: move |_| onmode.call(false), "Live" }
                            button { class: if smoother { "mode selected" } else { "mode" }, aria_pressed: smoother, aria_label: "Smooth: a larger playback buffer", onclick: move |_| onmode.call(true), "Smooth" }
                        }
                    }
                    if let Some(channel) = current {
                        div { class: "settings-section backup-section",
                            h3 { "Backup stream" }
                            div { class: "playback-option-row",
                                i { class: "backup-art", aria_hidden: true, Icon { d: LIVE_TV } }
                                span { title: "{channel.name}", "{channel.name}" }
                                button { class: "option-remove", aria_label: "Remove backup stream", title: "Remove backup", onclick: move |_| onbackup.call(None), Icon { d: CLOSE } }
                            }
                        }
                    }
                }
            }
    }
}
