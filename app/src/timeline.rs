//! Whole-title duration must not come from a converted stream's growing fragment.
pub fn duration(converted: bool, known: Option<f64>, hint: Option<u64>, browser: f64) -> f64 {
    let valid = |d: f64| d.is_finite() && d > 0.0;
    if let Some(d) = known.filter(|d| valid(*d)) {
        return d;
    }
    if converted {
        hint.filter(|d| *d > 0).map_or(0.0, |d| d as f64)
    } else if valid(browser) {
        browser
    } else {
        hint.filter(|d| *d > 0).map_or(0.0, |d| d as f64)
    }
}

/// Changing engines or reloading a converted segment cannot erase a known whole-title length.
pub fn retain(previous: f64, reported: f64) -> f64 {
    [reported, previous]
        .into_iter()
        .find(|d| d.is_finite() && *d > 0.0)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converted_fragments_never_shorten_the_movie() {
        for fragment in [7.0, 14.0, f64::INFINITY, f64::NAN] {
            assert_eq!(duration(true, Some(7200.0), None, fragment), 7200.0);
            assert_eq!(duration(true, None, Some(7200), fragment), 7200.0);
        }
        assert_eq!(duration(true, None, None, 7.0), 0.0);
    }
    #[test]
    fn indexed_engine_wins_and_native_files_use_their_own_duration() {
        assert_eq!(duration(false, Some(7200.0), Some(7000), 7.0), 7200.0);
        assert_eq!(duration(false, None, Some(7000), 7200.0), 7200.0);
        assert_eq!(duration(false, None, Some(7000), f64::NAN), 7000.0);
    }
    #[test]
    fn native_probe_survives_missing_or_fragment_browser_metadata() {
        for browser in [f64::NAN, f64::INFINITY, 0.0, 7.0] {
            assert_eq!(duration(false, Some(7200.0), None, browser), 7200.0);
        }
        assert_eq!(duration(false, Some(0.0), Some(7200), f64::NAN), 7200.0);
        assert_eq!(duration(false, Some(f64::NAN), None, 7200.0), 7200.0);
    }
    #[test]
    fn changing_engines_does_not_erase_a_known_length() {
        for missing in [0.0, f64::NAN, f64::INFINITY] {
            assert_eq!(retain(7200.0, missing), 7200.0);
        }
        assert_eq!(retain(0.0, 7200.0), 7200.0);
        assert_eq!(retain(f64::NAN, 0.0), 0.0);
    }
}
