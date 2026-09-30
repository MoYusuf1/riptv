//! Small, allocation-conscious reader for IPTV M3U lists. An HLS manifest is one channel, not a
//! list of its media segments. No EPG or media parsing belongs here.

use std::collections::HashMap;

use crate::{Category, Error, LiveStream, Result, Url};

pub(super) const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_CHANNELS: usize = 100_000;

#[derive(Debug)]
pub(super) struct Playlist {
    pub categories: Vec<Category>,
    pub channels: Vec<LiveStream>,
    pub urls: Vec<Url>,
}

fn attribute<'a>(metadata: &'a str, key: &str) -> Option<&'a str> {
    let start = metadata
        .match_indices(key)
        .find(|(i, _)| *i == 0 || metadata.as_bytes()[i - 1].is_ascii_whitespace())?
        .0
        + key.len();
    let value = metadata[start..].strip_prefix('=')?;
    let value = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split('"').next()?
    } else {
        value.split_whitespace().next()?
    };
    (!value.is_empty()).then_some(value)
}

fn metadata(line: &str) -> (&str, &str) {
    let mut quoted = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => quoted = !quoted,
            ',' if !quoted => return (&line[..i], line[i + 1..].trim()),
            _ => {}
        }
    }
    (line, "")
}

fn stream_url(base: &Url, line: &str) -> Option<Url> {
    let url = Url::parse(line).or_else(|_| base.join(line)).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

pub(super) fn parse(bytes: &[u8], source: &Url) -> Result<Playlist> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Playlist("file is not UTF-8"))?;
    let text = text.trim_start_matches('\u{feff}');
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(Error::Playlist("expected an M3U or M3U8 file"));
    }
    // HLS also uses #EXTINF, but its entries are segments, not channels.
    if text
        .lines()
        .any(|line| line.trim_start().starts_with("#EXT-X-"))
    {
        return Ok(Playlist {
            categories: vec![],
            channels: vec![LiveStream {
                stream_id: 1,
                name: "Live stream".into(),
                stream_icon: None,
                epg_channel_id: None,
                category_id: None,
                tv_archive: None,
            }],
            urls: vec![source.clone()],
        });
    }

    let mut playlist = Playlist {
        categories: vec![],
        channels: vec![],
        urls: vec![],
    };
    let mut group_ids = HashMap::<String, u64>::new();
    let mut pending = None;
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
        if let Some(info) = line.strip_prefix("#EXTINF:") {
            pending = Some(metadata(info));
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let Some((meta, display)) = pending.take() else {
            continue;
        };
        let Some(url) = stream_url(source, line) else {
            continue;
        };
        if playlist.channels.len() == MAX_CHANNELS {
            return Err(Error::Playlist("too many channels"));
        }
        let name = if display.is_empty() {
            attribute(meta, "tvg-name").unwrap_or("Live stream")
        } else {
            display
        };
        // Public lists often tag one channel with several groups ("News;Public"). Choose one
        // useful category instead of turning every combination into a separate sidebar entry.
        let group = attribute(meta, "group-title").and_then(|groups| {
            groups
                .split(';')
                .map(str::trim)
                .find(|g| !g.is_empty() && !g.eq_ignore_ascii_case("public"))
                .or_else(|| groups.split(';').map(str::trim).find(|g| !g.is_empty()))
        });
        let category_id = group.map(|group| {
            if let Some(id) = group_ids.get(group) {
                return *id;
            }
            let id = playlist.categories.len() as u64 + 1;
            playlist.categories.push(Category {
                category_id: id,
                category_name: group.to_owned(),
            });
            group_ids.insert(group.to_owned(), id);
            id
        });
        let stream_icon = attribute(meta, "tvg-logo")
            .filter(|logo| Url::parse(logo).is_ok_and(|u| matches!(u.scheme(), "http" | "https")))
            .map(str::to_owned);
        playlist.channels.push(LiveStream {
            stream_id: playlist.urls.len() as u64 + 1,
            name: name.to_owned(),
            stream_icon,
            epg_channel_id: None,
            category_id,
            tv_archive: None,
        });
        playlist.urls.push(url);
    }
    if playlist.channels.is_empty() {
        return Err(Error::Playlist("no playable http(s) channels found"));
    }
    Ok(playlist)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_channels_and_resolves_relative_urls() {
        let source = Url::parse("https://example.com/lists/tv.m3u").unwrap();
        let data = b"#EXTM3U\n#EXTINF:-1 tvg-name=\"News One\" tvg-logo=\"https://img.test/a.png\" group-title=\"News, Live\",News One\n../one.m3u8\n#EXTINF:-1 group-title=\"News, Live\",News Two\nhttps://example.net/two.ts\n#EXTINF:-1 group-title=\"Sports\",Three\nftp://example.com/bad\n";
        let p = parse(data, &source).unwrap();
        assert_eq!(p.categories.len(), 1);
        assert_eq!(p.categories[0].category_name, "News, Live");
        assert_eq!(p.channels.len(), 2);
        assert_eq!(p.channels[0].category_id, Some(1));
        assert_eq!(p.channels[1].category_id, Some(1));
        assert_eq!(
            p.channels[0].stream_icon.as_deref(),
            Some("https://img.test/a.png")
        );
        assert_eq!(p.urls[0].as_str(), "https://example.com/one.m3u8");
    }

    #[test]
    fn hls_segments_are_not_channels() {
        let source = Url::parse("https://example.com/live/index.m3u8").unwrap();
        let p = parse(
            b"#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\nsegment.ts\n",
            &source,
        )
        .unwrap();
        assert_eq!(p.channels.len(), 1);
        assert_eq!(p.urls[0], source);
    }

    #[test]
    fn public_playlist_tags_keep_the_useful_category() {
        let source = Url::parse("https://example.com/public.m3u").unwrap();
        let p = parse(b"#EXTM3U\n#EXTINF:-1 group-title=\"Public;Weather\",Camera\nhttps://example.com/camera.m3u8\n", &source).unwrap();
        assert_eq!(p.categories[0].category_name, "Weather");
    }

    #[test]
    fn unquoted_attributes_work() {
        let source = Url::parse("https://example.com/tv.m3u").unwrap();
        let p = parse(b"#EXTM3U\n#EXTINF:-1 group-title=News tvg-name=Bulletin,\nhttps://example.com/live.m3u8\n", &source).unwrap();
        assert_eq!(p.channels[0].name, "Bulletin");
        assert_eq!(p.categories[0].category_name, "News");
    }

    #[test]
    fn rejects_non_m3u_or_empty_lists() {
        let source = Url::parse("https://example.com/list.m3u").unwrap();
        assert!(parse(b"<html>error</html>", &source).is_err());
        assert!(parse(b"#EXTM3U\n", &source).is_err());
    }
}
