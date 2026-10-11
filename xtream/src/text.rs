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

/// A title without the provider's tag in front ("EN - Martha Bakes", "|US| Dune", "[4K] Dune",
/// "FHD: Dune"), for showing; search still uses the whole name. Only tags that are clearly tags
/// are taken off: a two-letter language or country code, or a quality marker, so "24: Live
/// Another Day" and "UFC: Fight Night" stay as they are.
pub fn display_title(title: &str) -> &str {
    const QUALITY: [&str; 9] = ["4K", "UHD", "FHD", "HD", "SD", "HEVC", "MULTI", "VIP", "8K"];
    let is_tag = |t: &str| {
        let t = t.trim();
        (t.len() == 2 && t.bytes().all(|b| b.is_ascii_uppercase())) || QUALITY.contains(&t)
    };
    let t = title.trim();
    // "|EN| Name", "[EN] Name", "(EN) Name"
    for (open, close) in [('|', '|'), ('[', ']'), ('(', ')')] {
        if let Some(rest) = t.strip_prefix(open)
            && let Some((tag, name)) = rest.split_once(close)
            && is_tag(tag)
            && !name.trim().is_empty()
        {
            return name.trim_start_matches([' ', '-', ':', '|']).trim();
        }
    }
    // "EN - Name", "EN: Name", "EN| Name", "EN | Name"
    for sep in [" - ", ": ", "| ", " | ", " – "] {
        if let Some((tag, name)) = t.split_once(sep)
            && is_tag(tag)
            && !name.trim().is_empty()
        {
            return name.trim();
        }
    }
    t
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

    #[test]
    fn provider_tags_come_off_titles_and_nothing_else() {
        use super::display_title as show;
        for (title, shown) in [
            ("EN - Martha Bakes", "Martha Bakes"),
            ("|US| Dune (2021)", "Dune (2021)"),
            ("[4K] Dune", "Dune"),
            ("FHD: Dune", "Dune"),
            ("UK | The Office", "The Office"),
            ("24: Live Another Day", "24: Live Another Day"),
            ("UFC: Fight Night", "UFC: Fight Night"),
            ("Mission: Impossible", "Mission: Impossible"),
            ("Spider-Man - Far From Home", "Spider-Man - Far From Home"),
            ("EN - ", "EN -"),
            ("Dune", "Dune"),
        ] {
            assert_eq!(show(title), shown, "{title}");
        }
    }
}
