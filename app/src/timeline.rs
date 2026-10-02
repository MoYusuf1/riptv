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
}
