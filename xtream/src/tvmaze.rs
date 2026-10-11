//! What a provider leaves out of a series (episode pictures and summaries, cast photos), from
//! TVmaze's free public API: one request per show, with its episodes and cast embedded. Only the
//! show's name is sent. The data is CC BY-SA, so wherever it shows, TVmaze is credited.
use crate::{Season, Url, has_episode_code};
use serde::Deserialize;

/// What TVmaze knows about one show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guide {
    /// The show's page on TVmaze, for the credit.
    pub page: String,
    pub plot: Option<String>,
    pub poster: Option<String>,
    pub episodes: Vec<GuideEpisode>,
    pub cast: Vec<Person>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuideEpisode {
    pub season: u64,
    pub number: u64,
    pub name: Option<String>,
    pub plot: Option<String>,
    pub image: Option<String>,
    pub date: Option<String>,
    pub runtime_secs: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Person {
    pub name: String,
    pub photo: Option<String>,
    pub role: Option<String>,
}

/// The TVmaze lookup for `title`: the show, with its episodes and cast.
pub fn search_url(title: &str) -> Url {
    let mut url = Url::parse("https://api.tvmaze.com/singlesearch/shows").expect("static URL");
    url.query_pairs_mut()
        .append_pair("q", &search_name(title))
        .append_pair("embed[]", "episodes")
        .append_pair("embed[]", "cast");
    url
}

/// An IPTV listing's name as a show's name: without the language or quality tag in front
/// ("EN - ", "|US| ", "4K: "), anything in brackets, or a trailing year.
pub fn search_name(title: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in title.chars() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth -= 1,
            _ if depth <= 0 => out.push(c),
            _ => {}
        }
    }
    let mut name = out.trim().trim_matches('|').trim();
    // A short tag before a separator: "EN - Name", "US| Name", "4K: Name".
    for sep in [" - ", "| ", ": ", " | "] {
        if let Some((tag, rest)) = name.split_once(sep)
            && (1..=4).contains(&tag.trim().len())
            && tag
                .trim()
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        {
            name = rest.trim();
        }
    }
    let name = match name.rsplit_once(' ') {
        Some((rest, year))
            if year.len() == 4
                && year.starts_with(['1', '2'])
                && year.chars().all(|c| c.is_ascii_digit()) =>
        {
            rest
        }
        _ => name,
    };
    name.trim().to_owned()
}

/// TVmaze's answer, if it is the show asked for as `title`: the same name (case and punctuation
/// aside), and, when the provider gives a `year`, a premiere within a year of it, so a near-miss
/// or a same-named show from another decade isn't taken for it.
pub fn parse(body: &[u8], title: &str, year: Option<u32>) -> Option<Guide> {
    let show: Show = serde_json::from_slice(body).ok()?;
    let key = |name: &str| {
        let s: String = search_name(name)
            .to_lowercase()
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        s.strip_prefix("the").map(str::to_owned).unwrap_or(s)
    };
    if key(&show.name) != key(title) {
        return None;
    }
    let premiered = show
        .premiered
        .as_deref()
        .and_then(|d| d.get(..4)?.parse::<u32>().ok());
    if let (Some(a), Some(b)) = (year, premiered)
        && a.abs_diff(b) > 1
    {
        return None;
    }
    let embedded = show.embedded.unwrap_or_default();
    Some(Guide {
        page: show.url,
        plot: show.summary.as_deref().and_then(text),
        poster: show.image.and_then(Image::best),
        episodes: embedded
            .episodes
            .into_iter()
            .filter_map(|e| {
                Some(GuideEpisode {
                    season: e.season?,
                    number: e.number?,
                    name: e.name.filter(|n| !n.trim().is_empty()),
                    plot: e.summary.as_deref().and_then(text),
                    image: e.image.and_then(Image::best),
                    date: e.airdate.filter(|d| !d.is_empty()),
                    runtime_secs: e.runtime.filter(|m| *m > 0).map(|m| m * 60),
                })
            })
            .collect(),
        cast: embedded
            .cast
            .into_iter()
            .map(|c| Person {
                name: c.person.name,
                photo: c.person.image.and_then(Image::best),
                role: c.character.map(|c| c.name).filter(|r| !r.trim().is_empty()),
            })
            .collect(),
    })
}

/// Fill in what the provider left out, episode by episode (by season and episode number). What
/// the provider did send always stays.
pub fn fill(seasons: &mut [Season], guide: &Guide) {
    for season in seasons {
        for (n, ep) in season.episodes.iter_mut().enumerate() {
            let number = ep.episode_num.unwrap_or(n as u64 + 1);
            let Some(g) = guide
                .episodes
                .iter()
                .find(|g| g.season == season.number && g.number == number)
            else {
                continue;
            };
            let info = &mut ep.info;
            let title = ep.title.trim();
            let named = info.name.is_some() || !(title.is_empty() || has_episode_code(title));
            if !named {
                info.name.clone_from(&g.name);
            }
            info.plot = info.plot.take().or_else(|| g.plot.clone());
            info.image = info
                .image
                .take()
                .filter(|i| i.starts_with("http"))
                .or_else(|| g.image.clone());
            info.release_date = info.release_date.take().or_else(|| g.date.clone());
            info.runtime_secs = info.runtime_secs.or(g.runtime_secs);
        }
    }
}

/// TVmaze's summaries are HTML (`<p>…</p>`, `<b>`); as plain text, entities decoded.
fn text(html: &str) -> Option<String> {
    let mut out = String::new();
    let mut tag = false;
    for c in html.chars() {
        match c {
            '<' => tag = true,
            '>' => tag = false,
            _ if !tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    let out = out.trim();
    (!out.is_empty()).then(|| out.to_owned())
}

#[derive(Deserialize)]
struct Show {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    premiered: Option<String>,
    summary: Option<String>,
    image: Option<Image>,
    #[serde(rename = "_embedded")]
    embedded: Option<Embedded>,
}

#[derive(Deserialize, Default)]
struct Embedded {
    #[serde(default)]
    episodes: Vec<Episode>,
    #[serde(default)]
    cast: Vec<Credit>,
}

#[derive(Deserialize)]
struct Episode {
    season: Option<u64>,
    number: Option<u64>,
    name: Option<String>,
    summary: Option<String>,
    image: Option<Image>,
    airdate: Option<String>,
    runtime: Option<u64>,
}

#[derive(Deserialize)]
struct Credit {
    person: Named,
    character: Option<Named>,
}

#[derive(Deserialize)]
struct Named {
    name: String,
    #[serde(default)]
    image: Option<Image>,
}

#[derive(Deserialize)]
struct Image {
    medium: Option<String>,
    original: Option<String>,
}

impl Image {
    /// The medium size: what a card or a portrait is shown at.
    fn best(self) -> Option<String> {
        self.medium
            .or(self.original)
            .filter(|u| u.starts_with("http"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Episode as Ep, EpisodeInfo};
    use serde_json::json;

    #[test]
    fn listing_names_become_show_names() {
        for (listing, show) in [
            ("EN - Martha Bakes", "Martha Bakes"),
            ("|US| Martha Knows Best (2020)", "Martha Knows Best"),
            ("4K: The Office [Multi-Sub]", "The Office"),
            (
                "Martha Gets Down and Dirty 2021",
                "Martha Gets Down and Dirty",
            ),
            ("Mythbusters - The Search", "Mythbusters - The Search"),
            ("24", "24"),
        ] {
            assert_eq!(search_name(listing), show, "{listing}");
        }
        assert!(
            search_url("EN - Martha Bakes")
                .as_str()
                .contains("q=Martha+Bakes")
        );
    }

    fn show() -> Vec<u8> {
        serde_json::to_vec(&json!({
            "name": "Martha Bakes", "url": "https://www.tvmaze.com/shows/16450/martha-bakes", "premiered": "2011-01-31",
            "summary": "<p>Martha <b>bakes</b> &amp; teaches.</p>", "image": {"medium": "https://s/p.jpg"},
            "_embedded": {
                "episodes": [
                    {"season": 1, "number": 1, "name": "Yellow Cake", "airdate": "2011-01-31", "runtime": 30,
                     "image": {"medium": "https://s/e1.jpg"}, "summary": "<p>A simple cake.</p>"},
                    {"season": 1, "number": 2, "name": "Pies", "airdate": "", "runtime": null, "image": null, "summary": null}
                ],
                "cast": [{"person": {"name": "Martha Stewart", "image": {"medium": "https://s/m.jpg"}},
                          "character": {"name": "Host"}},
                         {"person": {"name": "Guest", "image": null}, "character": null}]
            }
        }))
        .unwrap()
    }

    #[test]
    fn a_show_is_read_and_checked_against_the_year() {
        let g = parse(&show(), "EN - Martha Bakes", Some(2011)).unwrap();
        assert_eq!(g.plot.as_deref(), Some("Martha bakes & teaches."));
        assert_eq!(g.episodes.len(), 2);
        assert_eq!(g.episodes[0].runtime_secs, Some(1800));
        assert_eq!(g.episodes[1].date, None, "an empty airdate is no date");
        assert_eq!(g.cast[0].photo.as_deref(), Some("https://s/m.jpg"));
        assert_eq!(
            (g.cast[1].photo.as_ref(), g.cast[1].role.as_ref()),
            (None, None)
        );
        assert!(
            parse(&show(), "Martha Bakes", Some(2012)).is_some(),
            "a year apart is the same show"
        );
        assert!(
            parse(&show(), "Martha Bakes", Some(1995)).is_none(),
            "another decade is another show"
        );
        assert!(
            parse(&show(), "Martha Bakes!", None).is_some(),
            "punctuation aside"
        );
        assert!(
            parse(&show(), "Martha Stewart Living", None).is_none(),
            "a near miss isn't the show"
        );
        assert!(
            parse(b"null", "Martha Bakes", None).is_none(),
            "TVmaze had nothing"
        );
    }

    #[test]
    fn only_what_the_provider_left_out_is_filled_in() {
        let ep = |num: u64, title: &str, info: EpisodeInfo| Ep {
            id: num,
            episode_num: Some(num),
            title: title.into(),
            container_extension: None,
            info,
        };
        let mut seasons = vec![Season {
            number: 1,
            episodes: vec![
                ep(1, "Show - S01E01", EpisodeInfo::default()),
                ep(
                    2,
                    "My Pies",
                    EpisodeInfo {
                        plot: Some("Provider's plot".into()),
                        ..Default::default()
                    },
                ),
            ],
        }];
        fill(&mut seasons, &parse(&show(), "Martha Bakes", None).unwrap());
        let (a, b) = (&seasons[0].episodes[0], &seasons[0].episodes[1]);
        assert_eq!(a.name(), "Yellow Cake");
        assert_eq!(a.info.image.as_deref(), Some("https://s/e1.jpg"));
        assert_eq!(a.info.plot.as_deref(), Some("A simple cake."));
        assert_eq!(a.info.runtime_secs, Some(1800));
        assert_eq!(b.name(), "My Pies", "a real title stays");
        assert_eq!(
            b.info.plot.as_deref(),
            Some("Provider's plot"),
            "the provider's plot stays"
        );
    }
}
