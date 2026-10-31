//! A browser-wide color preference, independent of provider/profile credentials.
use crate::storage;
use dioxus::prelude::*;

const KEY: &str = "riptv.appearance";

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Palette {
    Blue,
    #[default]
    Teal,
    Violet,
    Amber,
}

impl Palette {
    pub const ALL: [Self; 4] = [Self::Blue, Self::Teal, Self::Violet, Self::Amber];
    pub fn name(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Violet => "Violet",
            Self::Amber => "Amber",
        }
    }
    pub fn accent(self) -> &'static str {
        match self {
            Self::Blue => "#a8c7fa",
            Self::Teal => "#80cbc4",
            Self::Violet => "#c4b5fd",
            Self::Amber => "#e7bd74",
        }
    }
    fn fill(self) -> &'static str {
        match self {
            Self::Blue => "#345fbe",
            Self::Teal => "#176a63",
            Self::Violet => "#6c4ac2",
            Self::Amber => "#80591e",
        }
    }
    pub fn css(self) -> String {
        format!(
            ":root{{--accent:{};--fill:{};--soft:color-mix(in srgb,var(--accent) 14%,transparent)}}",
            self.accent(),
            self.fill()
        )
    }
}

pub fn load() -> Palette {
    let saved = storage().and_then(|s| s.get_item(KEY).ok().flatten());
    Palette::ALL
        .into_iter()
        .find(|p| Some(p.name()) == saved.as_deref())
        .unwrap_or_default()
}

#[component]
pub fn Picker() -> Element {
    let mut palette = use_context::<Signal<Palette>>();
    let mut save_failed = use_signal(|| false);
    rsx! {
        fieldset { class: "palette-picker",
            legend { "Color" }
            div { class: "palette-options",
                for choice in Palette::ALL {
                    button {
                        class: "palette-choice",
                        aria_pressed: palette() == choice,
                        style: "--swatch:{choice.accent()}",
                        onclick: move |_| {
                            palette.set(choice);
                            save_failed.set(storage().is_none_or(|s| s.set_item(KEY, choice.name()).is_err()));
                        },
                        span { class: "palette-swatch", aria_hidden: "true" }
                        "{choice.name()}"
                    }
                }
            }
            if save_failed() { p { class: "palette-error", role: "status", "Color changed for this session. This preference couldn't be saved." } }
        }
    }
}
