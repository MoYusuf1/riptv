//! Cast photos for movies, which TVmaze doesn't cover, from Wikipedia's page summaries: only an
//! actor's name is sent. A photo is used only when the page is about a performer, so a namesake
//! (a basketball player, a page listing several people) doesn't stand in for them.
use crate::Url;
use serde::Deserialize;

/// The summary to ask for, in turn: the name itself, then the pages Wikipedia keeps for actors
/// who share a name with someone else.
pub fn summary_urls(name: &str) -> [Url; 3] {
    let page = name.trim().replace(' ', "_");
    [
        page.clone(),
        format!("{page}_(actor)"),
        format!("{page}_(actress)"),
    ]
    .map(|title| {
        let mut url =
            Url::parse("https://en.wikipedia.org/api/rest_v1/page/summary/").expect("static URL");
        url.path_segments_mut()
            .expect("http URL")
            .pop_if_empty()
            .push(&title);
        url
    })
}

/// The photo on a summary, if the page is one person who performs.
pub fn photo(body: &[u8]) -> Option<String> {
    let page: Summary = serde_json::from_slice(body).ok()?;
    let about = page.description?.to_lowercase();
    let performer = [
        "actor",
        "actress",
        "comedian",
        "presenter",
        "host",
        "filmmaker",
        "director",
        "singer",
        "rapper",
        "musician",
        "model",
        "performer",
        "personality",
        "voice",
        "stunt",
    ]
    .iter()
    .any(|w| about.contains(w));
    (page.kind == "standard" && performer)
        .then_some(page.thumbnail?.source)
        .filter(|s| s.starts_with("https://"))
}

#[derive(Deserialize)]
struct Summary {
    #[serde(rename = "type", default)]
    kind: String,
    description: Option<String>,
    thumbnail: Option<Thumbnail>,
}

#[derive(Deserialize)]
struct Thumbnail {
    source: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn page(kind: &str, about: &str, thumb: bool) -> Vec<u8> {
        let thumbnail = thumb.then(|| json!({"source": "https://upload.wikimedia.org/x.jpg"}));
        serde_json::to_vec(&json!({"type": kind, "description": about, "thumbnail": thumbnail}))
            .unwrap()
    }

    #[test]
    fn only_a_performer_s_photo_is_taken() {
        let ok = page("standard", "American actor and filmmaker (born 1956)", true);
        assert_eq!(
            photo(&ok).as_deref(),
            Some("https://upload.wikimedia.org/x.jpg")
        );
        assert!(photo(&page("standard", "American actress (born 1988)", true)).is_some());
        assert!(
            photo(&page("standard", "American basketball player", true)).is_none(),
            "a namesake"
        );
        assert!(
            photo(&page(
                "disambiguation",
                "Topics referred to by the same term",
                false
            ))
            .is_none()
        );
        assert!(
            photo(&page("standard", "American actor", false)).is_none(),
            "no photo"
        );
        assert!(photo(b"{\"title\":\"Not found.\"}").is_none());
    }

    #[test]
    fn names_become_summary_pages() {
        let [plain, actor, actress] = summary_urls("Chris Evans");
        assert_eq!(
            plain.as_str(),
            "https://en.wikipedia.org/api/rest_v1/page/summary/Chris_Evans"
        );
        assert!(actor.as_str().ends_with("/Chris_Evans_(actor)"));
        assert!(actress.as_str().ends_with("/Chris_Evans_(actress)"));
        let [odd, ..] = summary_urls("Zoë Saldaña/x");
        assert!(
            odd.as_str().ends_with("/Zo%C3%AB_Salda%C3%B1a%2Fx"),
            "{odd}"
        );
    }
}
