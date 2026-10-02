//! Small per-profile playback and browsing preferences. No stream URLs or passwords are stored.

use serde::{Deserialize, Serialize};

use crate::{shelves, storage};

const MAX_PINNED: usize = 100;

pub fn toggle_pin(pins: &mut Vec<u64>, id: u64) {
    if pins.contains(&id) {
        pins.retain(|pin| *pin != id);
    } else if pins.len() < MAX_PINNED {
        pins.push(id);
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub volume: u8,
    pub speed: f64,
    pub categories: [Option<u64>; 3],
    pub sorts: [u8; 3],
    /// Category IDs only, scoped to the signed-in profile and library section.
    pub pinned_categories: [Vec<u64>; 3],
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            volume: 100,
            speed: 1.0,
            categories: [None; 3],
            sorts: [0; 3],
            pinned_categories: Default::default(),
        }
    }
}

pub fn load() -> Preferences {
    storage()
        .and_then(|s| {
            s.get_item(&shelves::scoped_key("preferences"))
                .ok()
                .flatten()
        })
        .and_then(|json| serde_json::from_str::<Preferences>(&json).ok())
        .map(|mut p| {
            p.volume = p.volume.min(100);
            if !p.speed.is_finite() || !(0.5..=2.0).contains(&p.speed) {
                p.speed = 1.0;
            }
            for pins in &mut p.pinned_categories {
                pins.sort_unstable();
                pins.dedup();
                pins.truncate(MAX_PINNED);
            }
            p
        })
        .unwrap_or_default()
}

pub fn update(change: impl FnOnce(&mut Preferences)) {
    let mut p = load();
    change(&mut p);
    if let (Some(s), Ok(json)) = (storage(), serde_json::to_string(&p)) {
        let _ = s.set_item(&shelves::scoped_key("preferences"), &json);
    }
}
