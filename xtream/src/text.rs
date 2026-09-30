//! Text search the list screens use, without the allocation a naive version costs.

/// Whether `haystack` contains `needle_lower`, ignoring case. `needle_lower` must already be
/// lowercase (make it once per search, not once per title).
///
/// Comparing each of 30,000 titles by lowercasing it first allocates a string per title, per
/// keystroke. Plain ASCII, which most titles are, is compared in place; only a title with other
/// letters in it pays for full Unicode lowercasing.
pub fn contains_lowercase(haystack: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle_lower.is_ascii() {
        let needle = needle_lower.as_bytes();
        return haystack
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle));
    }
    haystack.to_lowercase().contains(needle_lower)
}

#[cfg(test)]
mod tests {
    use super::contains_lowercase as has;

    #[test]
    fn matches_what_lowercasing_first_would() {
        for (title, needle) in [
            ("The Crimson River 1999", "river"),
            ("The Crimson River 1999", "crimson river"),
            ("The Crimson River 1999", "1999"),
            ("The Crimson River 1999", "rivers"),
            ("The Crimson River 1999", "x"),
            ("Última Crimson", "última"),
            ("ÚLTIMA", "última"),
            ("Éternel", "eternel"),
            ("Straße", "straße"),
            ("İstanbul", "i̇stanbul"),
            ("abc", ""),
            ("", "a"),
            ("", ""),
            ("ab", "abc"),
        ] {
            assert_eq!(
                has(title, needle),
                title.to_lowercase().contains(needle),
                "{title:?} / {needle:?}"
            );
        }
    }
}
