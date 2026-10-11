//! Cast photos for movies, which TVmaze doesn't cover, from Wikipedia's page summaries, and each
//! actor's IMDb profile from Wikidata: only an actor's name (then their Wikidata item) is sent.
//! A page is used only when it is about a performer, so a namesake (a basketball player, a page
//! listing several people) doesn't stand in for them.
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

/// What Wikipedia knows of a performer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Performer {
    pub photo: Option<String>,
    /// Their Wikidata item (`Q2263`), where their IMDb id is kept.
    pub wikidata: Option<String>,
    /// Their IMDb profile, once looked up (see [`imdb_claims_url`]).
    pub imdb: Option<String>,
}

/// The performer a summary is about, if the page is one person who performs.
pub fn performer(body: &[u8]) -> Option<Performer> {
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
    (page.kind == "standard" && performer).then(|| Performer {
        photo: page
            .thumbnail
            .map(|t| t.source)
            .filter(|s| s.starts_with("https://")),
        wikidata: page
            .wikibase_item
            .filter(|q| q.starts_with('Q') && q[1..].chars().all(|c| c.is_ascii_digit())),
        imdb: None,
    })
}

/// Wikidata's IMDb id (property P345) for an item.
pub fn imdb_claims_url(item: &str) -> Url {
    let mut url = Url::parse("https://www.wikidata.org/w/api.php").expect("static URL");
    url.query_pairs_mut()
        .append_pair("action", "wbgetclaims")
        .append_pair("entity", item)
        .append_pair("property", "P345")
        .append_pair("format", "json");
    url
}

/// The IMDb profile in Wikidata's answer: a person's id is `nm` and digits.
pub fn imdb_profile(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let id = v["claims"]["P345"]
        .as_array()?
        .iter()
        .find_map(|c| c["mainsnak"]["datavalue"]["value"].as_str())?;
    (id.starts_with("nm") && id.len() > 2 && id[2..].chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("https://www.imdb.com/name/{id}/"))
}

/// IMDb's people search for a name: where an actor's link goes without a known profile.
pub fn imdb_search(name: &str) -> String {
    let mut url = Url::parse("https://www.imdb.com/find/").expect("static URL");
    url.query_pairs_mut()
        .append_pair("q", name.trim())
        .append_pair("s", "nm");
    url.to_string()
}

#[derive(Deserialize)]
struct Summary {
    #[serde(rename = "type", default)]
    kind: String,
    description: Option<String>,
    thumbnail: Option<Thumbnail>,
    wikibase_item: Option<String>,
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
        serde_json::to_vec(
            &json!({"type": kind, "description": about, "thumbnail": thumbnail,
                                   "wikibase_item": "Q2263"}),
        )
        .unwrap()
    }

    #[test]
    fn only_a_performer_s_photo_is_taken() {
        let ok = page("standard", "American actor and filmmaker (born 1956)", true);
        let tom = performer(&ok).unwrap();
        assert_eq!(
            tom.photo.as_deref(),
            Some("https://upload.wikimedia.org/x.jpg")
        );
        assert_eq!(tom.wikidata.as_deref(), Some("Q2263"));
        assert!(performer(&page("standard", "American actress (born 1988)", true)).is_some());
        assert!(
            performer(&page("standard", "American basketball player", true)).is_none(),
            "a namesake"
        );
        assert!(
            performer(&page(
                "disambiguation",
                "Topics referred to by the same term",
                false
            ))
            .is_none()
        );
        let no_photo = performer(&page("standard", "American actor", false)).unwrap();
        assert_eq!(
            no_photo.photo, None,
            "a performer without a photo still has a profile"
        );
        assert!(performer(b"{\"title\":\"Not found.\"}").is_none());
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

    #[test]
    fn imdb_profiles_come_from_wikidata_or_a_search() {
        let claims = br#"{"claims":{"P345":[{"mainsnak":{"datavalue":{"value":"nm0000158","type":"string"}}}]}}"#;
        assert_eq!(
            imdb_profile(claims).as_deref(),
            Some("https://www.imdb.com/name/nm0000158/")
        );
        assert_eq!(
            imdb_profile(
                br#"{"claims":{"P345":[{"mainsnak":{"datavalue":{"value":"tt0109830"}}}]}}"#
            ),
            None,
            "a title, not a person"
        );
        assert_eq!(imdb_profile(br#"{"claims":{}}"#), None);
        assert!(
            imdb_claims_url("Q2263")
                .as_str()
                .contains("entity=Q2263&property=P345")
        );
        assert_eq!(
            imdb_search("Tom Hanks"),
            "https://www.imdb.com/find/?q=Tom+Hanks&s=nm"
        );
    }
}
