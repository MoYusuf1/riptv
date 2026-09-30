//! Asks a picture host for the size that is actually shown.
//!
//! Panels point at full-size artwork (TMDB's `w600_and_h900_bestv2`, even `original`, which can be
//! thousands of pixels across), and a browser downloads and decodes all of it to paint a card
//! 140 pixels wide. TMDB serves any of its standard sizes from the same path, so asking for the
//! right one costs a string edit and saves most of the bytes, and most of the memory a decoded
//! bitmap takes (width x height x 4). Anything that isn't a TMDB image is left exactly as it is.

use std::borrow::Cow;

/// Where a picture is shown, which decides how big it needs to be.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Art {
    /// A poster in a grid (about 140 px wide, so 2x screens want 280).
    Thumb,
    /// A poster on a title's page.
    Poster,
    /// A full-width backdrop.
    Backdrop,
    /// An episode's still.
    Still,
}

impl Art {
    /// TMDB's size name, and the width it stands for.
    fn size(self) -> (&'static str, u32) {
        match self {
            Art::Thumb => ("w342", 342),
            Art::Poster => ("w500", 500),
            Art::Backdrop => ("w1280", 1280),
            Art::Still => ("w300", 300),
        }
    }
}

const PATH: &str = "image.tmdb.org/t/p/";

/// `url` for a picture shown as `art`: TMDB's size segment swapped for a smaller one if it names a
/// bigger one, and otherwise `url` itself.
pub fn sized(url: &str, art: Art) -> Cow<'_, str> {
    let Some(at) = url.find(PATH) else {
        return Cow::Borrowed(url);
    };
    let from = at + PATH.len();
    let Some(len) = url[from..].find('/') else {
        return Cow::Borrowed(url);
    };
    let current = &url[from..from + len];
    let (name, width) = art.size();
    // `w600_and_h900_bestv2` and its kin: the number after the `w` is the width.
    let current_width = match current {
        "original" => Some(u32::MAX),
        c if c.starts_with('w') => c[1..]
            .split(|ch: char| !ch.is_ascii_digit())
            .next()
            .and_then(|n| n.parse().ok()),
        _ => None,
    };
    match current_width {
        Some(w) if w > width => Cow::Owned(format!("{}{name}{}", &url[..from], &url[from + len..])),
        _ => Cow::Borrowed(url),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn big_tmdb_pictures_are_asked_for_at_the_size_shown() {
        let poster = "https://image.tmdb.org/t/p/w600_and_h900_bestv2/abc.jpg";
        assert_eq!(
            sized(poster, Art::Thumb),
            "https://image.tmdb.org/t/p/w342/abc.jpg"
        );
        assert_eq!(
            sized(poster, Art::Poster),
            "https://image.tmdb.org/t/p/w500/abc.jpg"
        );
        let original = "http://image.tmdb.org/t/p/original/xyz.jpg";
        assert_eq!(
            sized(original, Art::Backdrop),
            "http://image.tmdb.org/t/p/w1280/xyz.jpg"
        );
        assert_eq!(
            sized(original, Art::Still),
            "http://image.tmdb.org/t/p/w300/xyz.jpg"
        );
    }

    #[test]
    fn nothing_is_enlarged_and_nothing_else_is_touched() {
        // Already small enough: as it is (and not copied).
        for url in [
            "https://image.tmdb.org/t/p/w185/abc.jpg",
            "https://image.tmdb.org/t/p/w342/abc.jpg",
        ] {
            assert!(matches!(sized(url, Art::Thumb), Cow::Borrowed(u) if u == url));
        }
        // Other hosts, other shapes, and junk.
        for url in [
            "https://example.com/t/p/original/abc.jpg",
            "https://example.com/poster.jpg",
            "https://image.tmdb.org/t/p/original",
            "https://image.tmdb.org/t/p/",
            "https://image.tmdb.org/t/p/notasize/abc.jpg",
            "",
        ] {
            assert!(
                matches!(sized(url, Art::Thumb), Cow::Borrowed(u) if u == url),
                "{url}"
            );
        }
    }
}
