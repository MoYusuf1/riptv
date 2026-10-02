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

use crate::{Icon, add_playlist, explain, login, storage};

const KEY: &str = "riptv.profiles";

/// Avatar colours: a profile is its colour and its initial.
const COLORS: [&str; 8] = [
    "#e11d48", "#f97316", "#eab308", "#22c55e", "#06b6d4", "#3b82f6", "#8b5cf6", "#ec4899",
];
const PUBLIC_PLAYLIST: &str = "https://iptv-org.github.io/iptv/index.m3u";

const PLUS: &str = "M12 5v14M5 12h14";
const PENCIL: &str = "M4 20h4L19 9l-4-4L4 16v4zM14 6l4 4";

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
    /// Which of [`COLORS`]. Older profiles keep their saved colour.
    #[serde(default)]
    color: u8,
}

impl Profile {
    fn color(&self) -> &'static str {
        COLORS[usize::from(self.color) % COLORS.len()]
    }

    /// The first letter of the name, for the avatar.
    fn initial(&self) -> String {
        initial(&self.name)
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

fn initial(name: &str) -> String {
    name.trim()
        .chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".into(), |c| c.to_uppercase().collect())
}

fn random_index(len: usize) -> u8 {
    (js_sys::Math::random() * len as f64) as u8
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
    let mut screen = use_signal(|| Screen::Choose);
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
                    crate::shelves::set_scope(&p.id);
                    session.set(Some(client));
                }
                Err(e) => {
                    status.set(Some(explain(&e)));
                    busy.set(None);
                }
            }
        });
    };
    let free = Profile {
        id: "free".into(),
        name: "Public TV".into(),
        source: Source::Playlist,
        url: PUBLIC_PLAYLIST.into(),
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
                busy: busy().is_some(),
                status: status(),
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
                    screen.set(Screen::Choose);
                    profiles.set(list);
                },
            }
        };
    }

    let count = profiles().len();
    let spinner = |id: &str| busy().as_deref() == Some(id);
    rsx! {
        div { class: "login-page",
            main { class: "chooser",
                h1 { if managing() { "Edit profiles" } else { "Who's watching?" } }
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
                                if spinner(&p.id) { i { class: "spinner" } } else { "{p.initial()}" }
                                if managing() { span { class: "edit", Icon { d: PENCIL } } }
                            }
                            span { class: "name", "{p.name}" }
                        }
                    }
                    if !managing() {
                        button {
                            class: "profile",
                            style: "--i:{count}",
                            disabled: busy().is_some(),
                            onclick: { let free = free.clone(); move |_| connect(free.clone(), false) },
                            span { class: "avatar", style: "--c:#4a4146",
                                if spinner("free") { i { class: "spinner" } } else { Icon { d: crate::LIVE_TV } }
                            }
                            span { class: "name", "Public TV" }
                        }
                    }
                    button {
                        class: "profile",
                        style: "--i:{count + 1}",
                        disabled: busy().is_some(),
                        onclick: move |_| {
                            status.set(None);
                            screen.set(Screen::Form(None));
                        },
                        span { class: "avatar add", Icon { d: PLUS } }
                        span { class: "name", "Add" }
                    }
                }
                if count > 0 {
                    button { class: "text-btn", onclick: move |_| managing.set(!managing()), if managing() { "Done" } else { "Edit" } }
                }
                if let Some(msg) = status() { p { class: "err", "{msg}" } }
            }
        }
    }
}

/// The form for a profile, new or existing. It only hands the result back: what to do with it
/// (connect, save, delete) is the chooser's.
#[component]
fn ProfileForm(
    editing: Option<Profile>,
    busy: bool,
    status: Option<String>,
    onsubmit: EventHandler<Profile>,
    oncancel: EventHandler<()>,
    ondelete: EventHandler<String>,
) -> Element {
    let start = editing.clone();
    let mut source = use_signal(|| start.as_ref().map_or(Source::Xtream, |p| p.source));
    let mut name = use_signal(|| start.as_ref().map(|p| p.name.clone()).unwrap_or_default());
    let mut url = use_signal(|| start.as_ref().map(|p| p.url.clone()).unwrap_or_default());
    let mut user = use_signal(|| start.as_ref().map(|p| p.user.clone()).unwrap_or_default());
    let mut pass = use_signal(|| start.as_ref().map(|p| p.pass.clone()).unwrap_or_default());
    let mut color = use_signal(|| {
        start
            .as_ref()
            .map_or_else(|| random_index(COLORS.len()), |p| p.color)
    });
    let mut confirm = use_signal(|| false);
    let is_new = editing.is_none();
    let id = editing.as_ref().map_or_else(new_id, |p| p.id.clone());

    let delete_id = id.clone();
    rsx! {
        div { class: "login-page",
            form { class: "profile-form",
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
                div { class: "profile-form-heading",
                    button { r#type: "button", class: "icon-btn profile-back", aria_label: "Back", title: "Back", disabled: busy,
                        onclick: move |_| oncancel.call(()), Icon { d: crate::BACK } }
                    h1 { if is_new { "Add profile" } else { "Edit profile" } }
                }
                // Tap to change its colour.
                button {
                    r#type: "button",
                    class: "avatar",
                    style: "--c:{COLORS[usize::from(color()) % COLORS.len()]}",
                    aria_label: "Change colour",
                    onclick: move |_| color.set((color() + 1) % COLORS.len() as u8),
                    if name().trim().is_empty() { Icon { d: crate::USER } } else { "{initial(&name())}" }
                }
                div { class: "segmented", aria_label: "Source type",
                    button { r#type: "button", disabled: busy, class: if source() == Source::Xtream { "on" } else { "" }, onclick: move |_| source.set(Source::Xtream), "Xtream" }
                    button { r#type: "button", disabled: busy, class: if source() == Source::Playlist { "on" } else { "" }, onclick: move |_| source.set(Source::Playlist), "M3U" }
                }
                input { aria_label: "Profile name", placeholder: "Name", value: "{name}", oninput: move |e| name.set(e.value()) }
                if source() == Source::Xtream {
                    input { aria_label: "Server URL", placeholder: "Server", value: "{url}", oninput: move |e| url.set(e.value()) }
                    input { aria_label: "Username", autocomplete: "username", placeholder: "Username", value: "{user}", oninput: move |e| user.set(e.value()) }
                    input { r#type: "password", aria_label: "Password", autocomplete: "current-password", placeholder: "Password", value: "{pass}", oninput: move |e| pass.set(e.value()) }
                } else {
                    input { aria_label: "Playlist URL", placeholder: "Playlist URL", value: "{url}", oninput: move |e| url.set(e.value()) }
                }
                if let Some(msg) = status { p { class: "err", "{msg}" } }
                button { r#type: "submit", class: "primary", disabled: busy, if busy { "Connecting…" } else if is_new { "Connect" } else { "Save" } }
                if !is_new {
                    div { class: "row-actions",
                        button {
                            r#type: "button",
                            class: "text-btn danger",
                            onclick: move |_| {
                                if confirm() { ondelete.call(delete_id.clone()) } else { confirm.set(true) }
                            },
                            if confirm() { "Delete?" } else { "Delete" }
                        }
                    }
                }
            }
        }
    }
}
