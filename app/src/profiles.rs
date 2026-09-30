//! Saved profiles: the accounts you sign in with (an Xtream account, or an M3U/M3U8 playlist), kept
//! on this device and offered on a "Who's watching?" screen.
//!
//! Profiles live in the browser's `localStorage` for this address, as plain text: the password
//! included, which is what lets a tile sign in with one click. Nothing is sent anywhere but to the
//! provider it belongs to (through the local proxy, like everything else).

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use web_sys::js_sys;
use xtream::Client;

use crate::{Icon, RustMark, add_playlist, explain, login, storage};

const KEY: &str = "riptv.profiles";

/// Avatar colours, chosen on the form.
const COLORS: [&str; 8] = [
    "#e11d48", "#f97316", "#eab308", "#22c55e", "#06b6d4", "#3b82f6", "#8b5cf6", "#ec4899",
];

const PLUS: &str = "M12 5v14M5 12h14";
const PENCIL: &str = "M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4";
const CHECK_MARK: &str = "M5 12l5 5 9-10";

#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Source {
    Xtream,
    Playlist,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
struct Profile {
    id: String,
    name: String,
    source: Source,
    /// The server (Xtream) or the list's address (playlist).
    url: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    pass: String,
    /// Which of [`COLORS`].
    #[serde(default)]
    color: u8,
}

impl Profile {
    fn color(&self) -> &'static str {
        COLORS[usize::from(self.color) % COLORS.len()]
    }

    fn initial(&self) -> String {
        self.name
            .chars()
            .next()
            .map_or_else(|| "?".into(), |c| c.to_uppercase().to_string())
    }

    async fn connect(&self) -> xtream::Result<Client> {
        match self.source {
            Source::Xtream => login(&self.url, &self.user, &self.pass).await,
            Source::Playlist => add_playlist(&self.url).await,
        }
    }
}

fn load() -> Vec<Profile> {
    storage()
        .and_then(|s| s.get_item(KEY).ok().flatten())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn store(list: &[Profile]) {
    if let Some(s) = storage()
        && let Ok(json) = serde_json::to_string(list)
    {
        let _ = s.set_item(KEY, &json);
    }
}

fn new_id() -> String {
    format!(
        "{:x}{:x}",
        js_sys::Date::now() as u64,
        (js_sys::Math::random() * 1e9) as u64
    )
}

#[derive(Clone, PartialEq)]
enum Screen {
    Choose,
    /// The form, for a new profile or (with its id) an existing one.
    Form(Option<String>),
}

/// Who is watching: the saved profiles to pick from, or the form to make the first one.
#[component]
pub fn Login() -> Element {
    let mut session = use_context::<Signal<Option<Client>>>();
    let mut account = use_context::<Signal<String>>();
    let mut profiles = use_signal(load);
    let mut screen = use_signal(|| {
        if profiles.peek().is_empty() {
            Screen::Form(None)
        } else {
            Screen::Choose
        }
    });
    let mut managing = use_signal(|| false);
    // The profile being connected, and what went wrong with the last attempt.
    let mut busy = use_signal(|| None::<String>);
    let mut status = use_signal(|| None::<String>);

    // Sign in with `p`; `save` it too once that works (a new profile is kept only if it does).
    let connect = move |p: Profile, save: bool| {
        spawn(async move {
            busy.set(Some(p.id.clone()));
            status.set(None);
            match p.connect().await {
                Ok(client) => {
                    if save {
                        let mut list = profiles();
                        match list.iter_mut().find(|q| q.id == p.id) {
                            Some(q) => *q = p.clone(),
                            None => list.push(p.clone()),
                        }
                        store(&list);
                    }
                    busy.set(None);
                    account.set(p.name.clone());
                    session.set(Some(client));
                }
                Err(e) => {
                    status.set(Some(explain(&e)));
                    busy.set(None);
                }
            }
        });
    };
    let demo = Profile {
        id: "demo".into(),
        name: "Demo".into(),
        source: Source::Xtream,
        url: "http://127.0.0.1:8081".into(),
        user: "demo".into(),
        pass: "demo".into(),
        color: 0,
    };

    let free = Profile {
        id: "free".into(),
        name: "Public TV".into(),
        source: Source::Playlist,
        url: "https://iptv-org.github.io/iptv/categories/public.m3u".into(),
        user: String::new(),
        pass: String::new(),
        color: 5,
    };
    if let Screen::Form(id) = screen() {
        let editing = id.and_then(|id| profiles().into_iter().find(|p| p.id == id));
        let form_key = editing.as_ref().map_or("new".to_string(), |p| p.id.clone());
        return rsx! {
            ProfileForm {
                key: "{form_key}",
                editing: editing.clone(),
                // The first profile has nowhere to go back to.
                can_cancel: !profiles().is_empty(),
                busy: busy().is_some(),
                status: status(),
                next_color: (profiles().len() % COLORS.len()) as u8,
                onsubmit: move |p: Profile| {
                    if editing.is_some() {
                        let mut list = profiles();
                        if let Some(q) = list.iter_mut().find(|q| q.id == p.id) {
                            *q = p;
                        }
                        store(&list);
                        profiles.set(list);
                        screen.set(Screen::Choose);
                    } else {
                        connect(p, true);
                    }
                },
                oncancel: move |_| {
                    status.set(None);
                    screen.set(Screen::Choose);
                },
                ondelete: move |id: String| {
                    let mut list = profiles();
                    list.retain(|p| p.id != id);
                    store(&list);
                    screen.set(if list.is_empty() { Screen::Form(None) } else { Screen::Choose });
                    profiles.set(list);
                },
                ondemo: { let demo = demo.clone(); move |_| connect(demo.clone(), false) },
                onfree: { let free = free.clone(); move |_| connect(free.clone(), false) },
            }
        };
    }

    let count = profiles().len();
    let saved_label = format!(
        "{count} profile{} on this device",
        if count == 1 { "" } else { "s" }
    );
    rsx! {
        div { class: "login-page who",
            // The picture half (the top of a phone; a faint backdrop on a desktop).
            header { class: "who-hero",
                div { class: "who-mark", RustMark {} }
                div { class: "lockup",
                    strong { "RIPTV" }
                    span { "Your TV, movies and shows" }
                }
                span { class: "chip", "{saved_label}" }
            }
            main { class: "who-panel",
                h1 {
                    if managing() {
                        "Manage Profiles"
                    } else {
                        span { class: "m", "Choose Your Profile" }
                        span { class: "d", "Who's watching?" }
                    }
                }
                div { class: "profiles",
                    for (n, p) in profiles().into_iter().enumerate() {
                        button {
                            key: "{p.id}",
                            class: "profile",
                            style: "--i:{n}",
                            disabled: busy().is_some(),
                            onclick: {
                                let p = p.clone();
                                move |_| {
                                    if managing() {
                                        status.set(None);
                                        screen.set(Screen::Form(Some(p.id.clone())));
                                    } else {
                                        connect(p.clone(), false);
                                    }
                                }
                            },
                            span { class: "avatar", style: "--c:{p.color()}",
                                if busy().as_deref() == Some(p.id.as_str()) { i { class: "spinner" } } else { "{p.initial()}" }
                                if managing() { span { class: "edit", Icon { d: PENCIL } } }
                            }
                            strong { "{p.name}" }
                            small { if p.source == Source::Xtream { "Xtream" } else { "Playlist" } }
                        }
                    }
                    button {
                        class: "profile add",
                        style: "--i:{count}",
                        disabled: busy().is_some(),
                        onclick: move |_| {
                            status.set(None);
                            screen.set(Screen::Form(None));
                        },
                        span { class: "avatar tool", Icon { d: PLUS } }
                        strong { "Add" }
                    }
                    // On a phone this is a tile like the others; a desktop has the button below.
                    button {
                        class: "profile edit-tile",
                        style: "--i:{count + 1}",
                        disabled: busy().is_some(),
                        onclick: move |_| managing.set(!managing()),
                        span { class: "avatar tool", if managing() { Icon { d: CHECK_MARK } } else { Icon { d: PENCIL } } }
                        strong { if managing() { "Done" } else { "Edit" } }
                    }
                }
                button { class: "manage", onclick: move |_| managing.set(!managing()), if managing() { "Done" } else { "Manage profiles" } }
                if let Some(msg) = status() { p { class: "err", "{msg}" } }
                small { class: "note", "Profiles are saved on this device, in this browser." }
            }
        }
    }
}

/// The form for a profile, new or existing. It only hands the result back: what to do with it
/// (connect, save, delete) is the chooser's.
#[component]
fn ProfileForm(
    editing: Option<Profile>,
    can_cancel: bool,
    busy: bool,
    status: Option<String>,
    next_color: u8,
    onsubmit: EventHandler<Profile>,
    oncancel: EventHandler<()>,
    ondelete: EventHandler<String>,
    ondemo: EventHandler<()>,
    onfree: EventHandler<()>,
) -> Element {
    let start = editing.clone();
    let mut source = use_signal(|| start.as_ref().map_or(Source::Xtream, |p| p.source));
    let mut name = use_signal(|| start.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let mut url = use_signal(|| start.as_ref().map(|p| p.url.clone()).unwrap_or_default());
    let mut user = use_signal(|| start.as_ref().map(|p| p.user.clone()).unwrap_or_default());
    let mut pass = use_signal(|| start.as_ref().map(|p| p.pass.clone()).unwrap_or_default());
    let mut color = use_signal(|| start.as_ref().map_or(next_color, |p| p.color));
    let mut confirm = use_signal(|| false);
    let is_new = editing.is_none();
    let id = editing.as_ref().map_or_else(new_id, |p| p.id.clone());

    let shown = name()
        .trim()
        .chars()
        .next()
        .map_or("?".to_string(), |c| c.to_uppercase().to_string());
    let delete_id = id.clone();
    rsx! {
        div { class: "login-page",
            form { class: "login",
                onsubmit: move |e| {
                    e.prevent_default();
                    let address = url().trim().to_string();
                    let chosen = name().trim().to_string();
                    let name = if !chosen.is_empty() {
                        chosen
                    } else if source() == Source::Playlist {
                        "Playlist".to_string()
                    } else {
                        xtream::Url::parse(&address)
                            .ok()
                            .and_then(|u| u.host_str().map(str::to_owned))
                            .unwrap_or_else(|| "Account".to_string())
                    };
                    onsubmit.call(Profile {
                        id: id.clone(),
                        name,
                        source: source(),
                        url: address,
                        user: if source() == Source::Xtream { user().trim().to_string() } else { String::new() },
                        pass: if source() == Source::Xtream { pass() } else { String::new() },
                        color: color(),
                    });
                },
                div { class: "brand", RustMark {} span { "RIPTV" } }
                h1 { if is_new { "Add profile" } else { "Edit profile" } }
                span { class: "avatar preview", style: "--c:{COLORS[usize::from(color()) % COLORS.len()]}", "{shown}" }
                div { class: "login-modes", aria_label: "Source type",
                    button { r#type: "button", disabled: busy, class: if source() == Source::Xtream { "on" } else { "" }, onclick: move |_| source.set(Source::Xtream), "Xtream" }
                    button { r#type: "button", disabled: busy, class: if source() == Source::Playlist { "on" } else { "" }, onclick: move |_| source.set(Source::Playlist), "M3U / M3U8" }
                }
                input { aria_label: "Profile name", placeholder: "Profile name", value: "{name}", oninput: move |e| name.set(e.value()) }
                if source() == Source::Xtream {
                    input { aria_label: "Server URL", placeholder: "Server URL", value: "{url}", oninput: move |e| url.set(e.value()) }
                    input { aria_label: "Username", autocomplete: "username", placeholder: "Username", value: "{user}", oninput: move |e| user.set(e.value()) }
                    input { r#type: "password", aria_label: "Password", autocomplete: "current-password", placeholder: "Password", value: "{pass}", oninput: move |e| pass.set(e.value()) }
                } else {
                    input { aria_label: "Playlist URL", placeholder: "Playlist URL", value: "{url}", oninput: move |e| url.set(e.value()) }
                }
                div { class: "swatches", role: "radiogroup", aria_label: "Colour",
                    for (i, c) in COLORS.iter().enumerate() {
                        button {
                            key: "{i}",
                            r#type: "button",
                            class: if usize::from(color()) == i { "swatch on" } else { "swatch" },
                            style: "--c:{c}",
                            role: "radio",
                            aria_checked: usize::from(color()) == i,
                            aria_label: "Colour {i + 1}",
                            onclick: move |_| color.set(i as u8),
                        }
                    }
                }
                button { r#type: "submit", disabled: busy, if busy { "Connecting…" } else if is_new { "Save and connect" } else { "Save" } }
                if can_cancel { button { r#type: "button", class: "ghost", disabled: busy, onclick: move |_| oncancel.call(()), "Cancel" } }
                if !is_new {
                    button {
                        r#type: "button",
                        class: "ghost danger",
                        onclick: move |_| {
                            if confirm() { ondelete.call(delete_id.clone()) } else { confirm.set(true) }
                        },
                        if confirm() { "Click again to delete this profile" } else { "Delete profile" }
                    }
                }
                if is_new && source() == Source::Xtream {
                    button { r#type: "button", class: "ghost", disabled: busy, onclick: move |_| ondemo.call(()), "Try the demo" }
                }
                if is_new && source() == Source::Playlist {
                    button { r#type: "button", class: "ghost", disabled: busy, onclick: move |_| onfree.call(()), "Try free channels" }
                }
                if let Some(msg) = status { p { class: "err", "{msg}" } }
                small { class: "note", "Saved on this device, in this browser, password included." }
            }
        }
    }
}
