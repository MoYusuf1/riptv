//! What a stream is, from its first bytes: the container, and for MPEG-TS the codecs its program
//! map announces. Enough to choose how to play it before playing it; shared by the app (which
//! decides) and the proxy (whose diagnostics report it). O(n) in the bytes given, no allocation.

/// The first 64 KiB is plenty: a transport stream repeats its program map every ~100 ms.
pub const SAMPLE: usize = 64 * 1024;

/// What the first bytes of a stream say about it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Sniff {
    /// `mpeg_ts`, `mp4`, `matroska`, `hls_or_m3u`, `html_or_xml` or `unknown`.
    pub container: &'static str,
    pub video: Option<&'static str>,
    pub audio: Option<&'static str>,
}

impl Sniff {
    pub fn of(bytes: &[u8]) -> Self {
        let container = container(bytes);
        let (video, audio) = if container == "mpeg_ts" {
            ts_codecs(bytes)
        } else {
            (None, None)
        };
        Self {
            container,
            video,
            audio,
        }
    }
}

pub fn container(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"#EXTM3U") {
        "hls_or_m3u"
    } else if bytes.len() > 8 && &bytes[4..8] == b"ftyp" {
        "mp4"
    } else if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        "matroska"
    } else if ts_aligned(bytes) {
        "mpeg_ts"
    } else if bytes.starts_with(b"<") {
        "html_or_xml"
    } else {
        "unknown"
    }
}

/// Three packets in step at the start: a transport stream (one sync byte alone is a coincidence).
fn ts_aligned(bytes: &[u8]) -> bool {
    bytes.len() > 376 && bytes[0] == 0x47 && bytes[188] == 0x47 && bytes[376] == 0x47
}

/// The video and audio codecs in a transport stream's first program map table.
pub fn ts_codecs(bytes: &[u8]) -> (Option<&'static str>, Option<&'static str>) {
    let mut found = (None, None);
    for packet in bytes.as_chunks::<188>().0 {
        // A section starts in this packet (payload-unit-start), and it has a payload.
        if packet[0] != 0x47 || packet[1] & 0x40 == 0 || packet[3] & 0x10 == 0 {
            continue;
        }
        let mut offset = 4;
        if packet[3] & 0x20 != 0 {
            offset += 1 + usize::from(packet[4]);
        }
        if offset >= 188 {
            continue;
        }
        offset += 1 + usize::from(packet[offset]); // pointer field
        if offset + 12 > 188 || packet[offset] != 0x02 {
            continue; // not a program map
        }
        let length =
            (usize::from(packet[offset + 1] & 0x0f) << 8) | usize::from(packet[offset + 2]);
        let end = (offset + 3 + length).saturating_sub(4).min(188);
        let info =
            (usize::from(packet[offset + 10] & 0x0f) << 8) | usize::from(packet[offset + 11]);
        offset += 12 + info;
        while offset + 5 <= end {
            let es_info =
                (usize::from(packet[offset + 3] & 0x0f) << 8) | usize::from(packet[offset + 4]);
            let descriptors = &packet[(offset + 5).min(end)..(offset + 5 + es_info).min(end)];
            match packet[offset] {
                0x1b => found.0 = Some("h264"),
                0x24 => found.0 = Some("hevc"),
                0x01 | 0x02 => found.0 = Some("mpeg2video"),
                0x0f => found.1 = found.1.or(Some("aac")),
                // AAC in LATM framing (DVB broadcasts): browsers don't decode it.
                0x11 => found.1 = found.1.or(Some("aac_latm")),
                0x03 | 0x04 => found.1 = found.1.or(Some("mp2")),
                0x81 => found.1 = found.1.or(Some("ac3")),
                0x87 => found.1 = found.1.or(Some("eac3")),
                // Private data: DVB says what it is in a descriptor (AC-3 0x6a, E-AC-3 0x7a).
                0x06 => {
                    if has_descriptor(descriptors, 0x7a) {
                        found.1 = found.1.or(Some("eac3"));
                    } else if has_descriptor(descriptors, 0x6a) {
                        found.1 = found.1.or(Some("ac3"));
                    }
                }
                _ => {}
            }
            offset += 5 + es_info;
        }
        if found.0.is_some() || found.1.is_some() {
            break;
        }
    }
    found
}

fn has_descriptor(mut list: &[u8], tag: u8) -> bool {
    while list.len() >= 2 {
        if list[0] == tag {
            return true;
        }
        list = list.get(2 + usize::from(list[1])..).unwrap_or_default();
    }
    false
}

/// How a standard (non-experimental) player should play a stream: given what it is and what the
/// browser decodes, as is, or converted by the proxy's ffmpeg (just the sound, or the video).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plan {
    /// The browser decodes both: play it directly.
    Browser,
    /// The picture plays but the sound doesn't (AC-3, E-AC-3, MP2): copy the video, convert the
    /// sound. Cheap: no video encoding.
    ConvertSound,
    /// The browser can't decode the video (HEVC here, MPEG-2): encode it as H.264.
    ConvertVideo,
}

impl Plan {
    /// What the proxy is asked to do with the video: `copy` or `transcode`.
    pub fn video(self) -> &'static str {
        match self {
            Self::ConvertVideo => "transcode",
            Self::Browser | Self::ConvertSound => "copy",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Browser, Self::ConvertSound, Self::ConvertVideo]
            .into_iter()
            .find(|p| p.name() == name)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::ConvertSound => "convert_sound",
            Self::ConvertVideo => "convert_video",
        }
    }
}

/// The plan for `sniffed`, where `decodes(mime)` says whether this browser plays that type. An
/// unknown codec is assumed playable: the player's own checks (no frames, no sound) still catch it.
pub fn plan(sniffed: &Sniff, decodes: impl Fn(&str) -> bool) -> Plan {
    let video_ok = match sniffed.video {
        Some("hevc") => decodes(r#"video/mp4; codecs="hvc1.1.6.L120.90""#),
        Some("mpeg2video") => false,
        _ => true,
    };
    let audio_ok = match sniffed.audio {
        Some("ac3") => decodes(r#"audio/mp4; codecs="ac-3""#),
        Some("eac3") => decodes(r#"audio/mp4; codecs="ec-3""#),
        Some("mp2" | "aac_latm") => false,
        _ => true,
    };
    match (video_ok, audio_ok) {
        (false, _) => Plan::ConvertVideo,
        (true, false) => Plan::ConvertSound,
        (true, true) => Plan::Browser,
    }
}

/// Which line of a playlist to follow: a master playlist's first variant (they share codecs), or
/// a media playlist's newest segment (the oldest may already have left a live server's window).
pub fn next_in(playlist: &str) -> Option<&str> {
    let mut uris = playlist
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'));
    if playlist.contains("#EXTINF") {
        uris.next_back()
    } else {
        uris.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-packet transport stream whose program map lists `streams` (type, descriptors).
    fn pmt(streams: &[(u8, &[u8])]) -> Vec<u8> {
        let mut section = vec![0x02, 0, 0, 0, 1, 0xc1, 0, 0, 0xe1, 0x00, 0xf0, 0x00];
        for (kind, descriptors) in streams {
            section.extend([*kind, 0xe1, 0x01, 0xf0, descriptors.len() as u8]);
            section.extend_from_slice(descriptors);
        }
        let length = section.len() - 3 + 4;
        section[1] = 0xb0 | (length >> 8) as u8;
        section[2] = length as u8;
        section.extend([0; 4]); // CRC, unchecked
        let mut packet = vec![0x47, 0x40, 0x01, 0x10, 0x00];
        packet.extend(section);
        packet.resize(188, 0xff);
        let mut ts = packet.clone();
        for _ in 0..2 {
            ts.extend([0x47, 0x1f, 0xff, 0x10]);
            ts.resize(ts.len() + 184, 0xff);
        }
        ts
    }

    #[test]
    fn program_maps_name_their_codecs() {
        let s = Sniff::of(&pmt(&[(0x1b, &[]), (0x81, &[])]));
        assert_eq!(
            (s.container, s.video, s.audio),
            ("mpeg_ts", Some("h264"), Some("ac3"))
        );
        let s = Sniff::of(&pmt(&[(0x24, &[]), (0x0f, &[])]));
        assert_eq!((s.video, s.audio), (Some("hevc"), Some("aac")));
        let s = Sniff::of(&pmt(&[(0x1b, &[]), (0x11, &[])]));
        assert_eq!(s.audio, Some("aac_latm"));
        // DVB-style E-AC-3: private data with an enhanced AC-3 descriptor.
        let s = Sniff::of(&pmt(&[
            (0x1b, &[]),
            (0x06, &[0x0a, 0x01, 0x00, 0x7a, 0x01, 0x00]),
        ]));
        assert_eq!(s.audio, Some("eac3"));
        // The first audio track is the one that plays.
        let s = Sniff::of(&pmt(&[(0x1b, &[]), (0x0f, &[]), (0x81, &[])]));
        assert_eq!(s.audio, Some("aac"));
    }

    #[test]
    fn containers_are_told_apart() {
        assert_eq!(container(b"#EXTM3U\n"), "hls_or_m3u");
        assert_eq!(container(b"\0\0\0\x18ftypisom"), "mp4");
        assert_eq!(container(b"<html>"), "html_or_xml");
        assert_eq!(container(&[0x47; 10]), "unknown");
    }

    #[test]
    fn a_live_playlist_is_followed_to_its_newest_segment() {
        assert_eq!(
            next_in("#EXTM3U\n#EXTINF:6,\na.ts\n#EXTINF:6,\nb.ts\n"),
            Some("b.ts")
        );
        assert_eq!(
            next_in(
                "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=2\nhigh.m3u8\n"
            ),
            Some("low.m3u8")
        );
    }

    fn sniffed(video: Option<&'static str>, audio: Option<&'static str>) -> Sniff {
        Sniff {
            container: "mpeg_ts",
            video,
            audio,
        }
    }

    #[test]
    fn each_stream_gets_the_cheapest_plan_that_plays() {
        let linux_chrome =
            |mime: &str| !mime.contains("hvc1") && !mime.contains("ac-3") && !mime.contains("ec-3");
        let everything = |_: &str| true;
        let h264_aac = sniffed(Some("h264"), Some("aac"));
        assert_eq!(plan(&h264_aac, linux_chrome), Plan::Browser);
        let surround = sniffed(Some("h264"), Some("eac3"));
        assert_eq!(plan(&surround, linux_chrome), Plan::ConvertSound);
        assert_eq!(plan(&surround, everything), Plan::Browser);
        let uhd = sniffed(Some("hevc"), Some("ac3"));
        assert_eq!(plan(&uhd, linux_chrome), Plan::ConvertVideo);
        assert_eq!(
            plan(&sniffed(Some("mpeg2video"), Some("mp2")), everything),
            Plan::ConvertVideo
        );
        assert_eq!(plan(&sniffed(None, None), linux_chrome), Plan::Browser);
        assert_eq!(Plan::ConvertSound.video(), "copy");
        assert_eq!(Plan::from_name("convert_video"), Some(Plan::ConvertVideo));
    }
}
