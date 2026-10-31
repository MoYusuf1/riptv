//! A browser-wide color preference, independent of provider/profile credentials.
use crate::storage;
use dioxus::prelude::*;

const KEY: &str = "riptv.appearance";

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Palette {
    #[default]
    Charcoal,
    Blue,
    Teal,
    Violet,
    Amber,
}

impl Palette {
    pub const ALL: [Self; 5] = [
        Self::Charcoal,
        Self::Blue,
        Self::Teal,
        Self::Violet,
        Self::Amber,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Charcoal => "Charcoal",
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Violet => "Violet",
            Self::Amber => "Amber",
        }
    }
    fn parse(saved: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|palette| palette.name() == saved)
    }
    // Each palette owns surfaces and text as well as its interactive colors.
    fn tokens(self) -> [&'static str; 13] {
        match self {
            Self::Charcoal => [
                "#111113",
                "#19191c",
                "#242427",
                "#35353b",
                "#f1f1f3",
                "#b2b2bc",
                "#96969f",
                "#303034",
                "#3e3e44",
                "rgba(25,25,28,.96)",
                "#c8c8d0",
                "#55555f",
                "#2d2d33",
            ],
            Self::Blue => [
                "#101620",
                "#182230",
                "#233246",
                "#30445c",
                "#edf3fc",
                "#b1c1d8",
                "#91a6c2",
                "#2c3b50",
                "#3c506b",
                "rgba(24,34,48,.96)",
                "#a8c7fa",
                "#345f9d",
                "#26394f",
            ],
            Self::Teal => [
                "#0e1918",
                "#162724",
                "#203833",
                "#2c4a44",
                "#e9f5f1",
                "#aecac2",
                "#8caea5",
                "#2a413b",
                "#39584f",
                "rgba(22,39,36,.96)",
                "#80cbc4",
                "#286b60",
                "#24433c",
            ],
            Self::Violet => [
                "#191320",
                "#261e30",
                "#362a43",
                "#49395a",
                "#f5eefb",
                "#c7b8d6",
                "#ab97bd",
                "#403149",
                "#554160",
                "rgba(38,30,48,.96)",
                "#c4b5fd",
                "#70549e",
                "#3e3051",
            ],
            Self::Amber => [
                "#1b160f",
                "#2a2218",
                "#3b3021",
                "#50412d",
                "#faf2e5",
                "#d1c0a5",
                "#b49c7c",
                "#443725",
                "#5b4930",
                "rgba(42,34,24,.96)",
                "#e7bd74",
                "#805e2b",
                "#463722",
            ],
        }
    }
    fn properties(self) -> String {
        [
            "bg", "panel", "row", "hover", "text", "dim", "faint", "hair", "line", "glass",
            "accent", "fill", "soft",
        ]
        .into_iter()
        .zip(self.tokens())
        .map(|(name, value)| format!("--{name}:{value};"))
        .collect()
    }
    pub fn css(self) -> String {
        format!(":root{{{}}}", self.properties())
    }
}

pub fn load() -> Palette {
    let saved = storage().and_then(|s| s.get_item(KEY).ok().flatten());
    saved
        .as_deref()
        .and_then(Palette::parse)
        .unwrap_or_default()
}

#[component]
pub fn Picker() -> Element {
    let mut palette = use_context::<Signal<Palette>>();
    let mut save_failed = use_signal(|| false);
    rsx! {
        fieldset { class: "palette-picker",
            legend { "Palette" }
            div { class: "palette-options",
                for choice in Palette::ALL {
                    button {
                        class: "palette-choice",
                        aria_pressed: palette() == choice,
                        style: choice.properties(),
                        onclick: move |_| {
                            palette.set(choice);
                            save_failed.set(storage().is_none_or(|s| s.set_item(KEY, choice.name()).is_err()));
                        },
                        span { class: "palette-preview", aria_hidden: "true",
                            span { class: "palette-preview-panel", span { class: "palette-preview-text" } span { class: "palette-preview-accent" } }
                        }
                        "{choice.name()}"
                    }
                }
            }
            button { class: "palette-reset", onclick: move |_| {
                palette.set(Palette::default());
                save_failed.set(storage().is_none_or(|s| s.remove_item(KEY).is_err()));
            }, "Reset to default" }
            if save_failed() { p { class: "palette-error", role: "status", "Palette changed for this session. This preference couldn't be saved." } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_names_and_default_are_stable() {
        assert_eq!(Palette::default(), Palette::Charcoal);
        for palette in Palette::ALL {
            assert_eq!(Palette::parse(palette.name()), Some(palette));
        }
        assert_eq!(Palette::parse("unknown"), None);
    }

    #[test]
    fn every_palette_sets_every_color_token() {
        for palette in Palette::ALL {
            let css = palette.css();
            for token in [
                "bg", "panel", "row", "hover", "text", "dim", "faint", "hair", "line", "glass",
                "accent", "fill", "soft",
            ] {
                assert!(css.contains(&format!("--{token}:")));
            }
            assert_eq!(css.matches(';').count(), 13);
        }
    }
}
