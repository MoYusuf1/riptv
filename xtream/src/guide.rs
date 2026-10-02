//! Turn provider EPG rows into a non-overlapping timeline. Panels often repeat entries or send
//! listings that extend into the next show's slot; neither should draw blocks on top of each other.

use crate::EpgListing;

pub struct Slot<'a> {
    pub start: u64,
    pub end: u64,
    pub listing: &'a EpgListing,
}

/// Reject a stale or far-future full table so the caller can try the provider's short EPG.
pub fn has_nearby(entries: &[EpgListing], now: u64) -> bool {
    entries
        .iter()
        .filter(|entry| !placeholder(&entry.title))
        .any(|entry| {
            matches!((entry.start_ts, entry.end_ts), (Some(start), Some(end))
            if end > now.saturating_sub(2 * 3600) && start < now + 6 * 3600)
        })
}

pub fn normalize(entries: &[EpgListing]) -> Vec<Slot<'_>> {
    let mut slots: Vec<_> = entries
        .iter()
        .filter(|listing| !placeholder(&listing.title))
        .filter_map(|listing| {
            let (start, end) = (listing.start_ts?, listing.end_ts?);
            (end > start).then_some(Slot {
                start,
                end,
                listing,
            })
        })
        .collect();
    // Prefer the longest listing when a provider sends several with the same start time.
    slots.sort_unstable_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    slots.dedup_by_key(|slot| slot.start);
    for i in 0..slots.len().saturating_sub(1) {
        slots[i].end = slots[i].end.min(slots[i + 1].start);
    }
    slots.retain(|slot| slot.end > slot.start);
    slots
}

/// Provider filler is not a programme and should not reserve a guide panel.
pub fn placeholder(title: &str) -> bool {
    let title = title.trim().to_ascii_uppercase();
    title.is_empty()
        || [
            "NO GUIDE",
            "NO EPG",
            "EPG NOT NEEDED",
            "GUIDE NOT NEEDED",
            "NO PROGRAM INFORMATION",
            "NO PROGRAMME INFORMATION",
        ]
        .iter()
        .any(|filler| title.contains(filler))
}

/// The programme at the requested instant, never a past or upcoming listing.
/// Use the same overlap rules as the full timeline.
pub fn current(entries: &[EpgListing], now: u64) -> Option<Slot<'_>> {
    normalize(entries)
        .into_iter()
        .find(|slot| slot.start <= now && now < slot.end)
}

/// Short programmes need a closer zoom, while long programmes can use a wider time window.
pub fn seconds_per_px(slots: &[Slot<'_>], lo: u64, hi: u64) -> u64 {
    let mut lengths: Vec<_> = slots
        .iter()
        .filter(|slot| slot.end > lo && slot.start < hi)
        .map(|slot| slot.end.saturating_sub(slot.start))
        .collect();
    if lengths.is_empty() {
        return 20;
    }
    lengths.sort_unstable();
    (lengths[lengths.len() / 2] / 140).clamp(8, 40)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(start: u64, end: u64) -> EpgListing {
        EpgListing {
            title: format!("{start}"),
            description: String::new(),
            start: String::new(),
            end: String::new(),
            start_ts: Some(start),
            end_ts: Some(end),
        }
    }

    #[test]
    fn duplicate_and_overlapping_rows_do_not_cover_each_other() {
        let rows = [
            listing(100, 300),
            listing(100, 150),
            listing(200, 400),
            listing(400, 400),
        ];
        let slots = normalize(&rows);
        assert_eq!(slots.len(), 2);
        assert_eq!((slots[0].start, slots[0].end), (100, 200));
        assert_eq!((slots[1].start, slots[1].end), (200, 400));
    }

    #[test]
    fn zoom_matches_programme_length() {
        let short = [listing(0, 1800), listing(1800, 3600)];
        let long = [listing(0, 7200), listing(7200, 14400)];
        assert!(
            seconds_per_px(&normalize(&short), 0, 3600)
                < seconds_per_px(&normalize(&long), 0, 14400)
        );
    }

    #[test]
    fn stale_table_needs_short_epg() {
        assert!(!has_nearby(&[listing(100, 200)], 10_000));
        assert!(has_nearby(&[listing(9_000, 10_500)], 10_000));
    }

    #[test]
    fn current_respects_boundaries_gaps_and_overlaps() {
        let rows = [listing(100, 300), listing(200, 250), listing(400, 500)];
        assert!(current(&rows, 99).is_none());
        assert_eq!(current(&rows, 100).unwrap().listing.title, "100");
        assert_eq!(current(&rows, 200).unwrap().listing.title, "200");
        assert!(current(&rows, 250).is_none());
        assert!(current(&rows, 500).is_none());
    }

    #[test]
    fn filler_does_not_reserve_guide_space() {
        let mut row = listing(100, 300);
        row.title = "No guide needed".into();
        assert!(!has_nearby(std::slice::from_ref(&row), 150));
        assert!(normalize(&[row]).is_empty());
        assert!(!placeholder("NFL: Steelers vs Browns"));
    }
}
