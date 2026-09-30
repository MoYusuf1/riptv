//! What the viewer has watched and set aside, kept per profile in the browser's storage (as plain
//! text, like the profiles): how far into each title they got, the titles they have started
//! ("Continue watching"), and the ones they have saved ("My List").
//!
//! Everything is keyed by the signed-in profile, so two providers that both have a "movie 12" don't
//! share progress.

use std::cell::RefCell;

use serde::{Deserialize, Serialize};

use crate::storage;

thread_local! {
    static SCOPE: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Whose data this is: the profile just signed in. (One thread, so a global is enough.)
pub fn set_scope(profile: &str) {
    SCOPE.with(|s| *s.borrow_mut() = profile.to_owned());
}

fn name(what: &str) -> String {
    SCOPE.with(|s| format!("riptv.{}.{what}", s.borrow()))
}

/// A movie, or a series (which stands for the episode being watched).
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Title {
    Movie,
    Series,
}

/// What a shelf needs to draw a title and open it again. Never a stream address: those carry the
/// account's credentials, and are made afresh from the id.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub kind: Title,
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub icon: Option<String>,
    /// A movie's container (`mkv`), which its address needs.
    #[serde(default)]
    pub ext: Option<String>,
    /// "S1 E3" for the episode a series entry is at.
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub at: f64,
    #[serde(default)]
    pub total: f64,
}

impl Entry {
    /// How much has been watched, 1 to 99; `None` if it can't be said.
    pub fn percent(&self) -> Option<u32> {
        percent_of(self.at, self.total)
    }
}

/// How many titles each shelf keeps.
const RECENT: usize = 24;
const LIST: usize = 200;

fn load(list: &str) -> Vec<Entry> {
    storage()
        .and_then(|s| s.get_item(&name(list)).ok().flatten())
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn store(list: &str, entries: &[Entry]) {
    if let Some(s) = storage()
        && let Ok(json) = serde_json::to_string(entries)
    {
        let _ = s.set_item(&name(list), &json);
    }
}

/// Puts `entry` first in `list` (replacing an earlier one of the same title).
fn promote(list: &str, entry: Entry, keep: usize) {
    let mut all = load(list);
    all.retain(|e| !(e.kind == entry.kind && e.id == entry.id));
    all.insert(0, entry);
    all.truncate(keep);
    store(list, &all);
}

fn remove(list: &str, kind: Title, id: u64) {
    let mut all = load(list);
    let before = all.len();
    all.retain(|e| !(e.kind == kind && e.id == id));
    if all.len() != before {
        store(list, &all);
    }
}

/// The titles started and not finished, latest first.
pub fn recent() -> Vec<Entry> {
    load("recent")
}

/// The titles saved to My List, latest first.
pub fn mine() -> Vec<Entry> {
    load("list")
}

pub fn is_mine(kind: Title, id: u64) -> bool {
    mine().iter().any(|e| e.kind == kind && e.id == id)
}

/// Saves `entry` to My List, or takes it off if it is there. Returns whether it is now on it.
pub fn toggle_mine(entry: Entry) -> bool {
    if is_mine(entry.kind, entry.id) {
        remove("list", entry.kind, entry.id);
        false
    } else {
        promote("list", entry, LIST);
        true
    }
}

fn progress_key(key: &str) -> String {
    name(&format!("at.{key}"))
}

/// How far the viewer got last time: (seconds in, seconds in all), the second 0 if not known.
/// `None` if they haven't started or have finished.
pub fn progress(key: &str) -> Option<(f64, f64)> {
    let saved = storage()?.get_item(&progress_key(key)).ok()??;
    // "754/7200".
    let (at, total) = saved.split_once('/').unwrap_or((&saved, "0"));
    let at: f64 = at.parse().ok().filter(|a| *a > 0.0)?;
    Some((at, total.parse().unwrap_or(0.0)))
}

/// Where the viewer stopped last time, in seconds (0 if not started or finished).
pub fn position(key: &str) -> f64 {
    progress(key).map_or(0.0, |p| p.0)
}

pub fn percent_of(at: f64, total: f64) -> Option<u32> {
    (total > 0.0).then(|| (at / total * 100.0).clamp(1.0, 99.0) as u32)
}

/// How much of a title has been watched, 1 to 99, for a progress bar.
pub fn percent(key: &str) -> Option<u32> {
    progress(key).and_then(|(at, total)| percent_of(at, total))
}

/// Remembers the position, and the title among those started, unless the viewer is at the very
/// start (a peek isn't watching) or has all but finished (which [`finish`] handles).
pub fn save(key: &str, at: f64, total: f64, entry: Option<&Entry>) {
    if total > 0.0 && at > total * 0.95 {
        return finish(key, entry);
    }
    if let Some(s) = storage() {
        if at < 30.0 {
            // Back at the start ("Start over"): there is nothing to resume. (A series stays: an
            // earlier episode may still be half watched.)
            let _ = s.remove_item(&progress_key(key));
            if let Some(e) = entry.filter(|e| e.kind == Title::Movie) {
                remove("recent", e.kind, e.id);
            }
            return;
        }
        let _ = s.set_item(
            &progress_key(key),
            &format!("{}/{}", at as u64, total as u64),
        );
    }
    if let Some(e) = entry {
        promote(
            "recent",
            Entry {
                at,
                total,
                ..e.clone()
            },
            RECENT,
        );
    }
}

/// The viewer watched it to the end: no longer to be continued.
pub fn finish(key: &str, entry: Option<&Entry>) {
    if let Some(s) = storage() {
        let _ = s.remove_item(&progress_key(key));
    }
    if let Some(e) = entry {
        remove("recent", e.kind, e.id);
    }
}
