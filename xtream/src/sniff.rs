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
    /// From the H.264 sequence header, when one is in the sample: whether the picture is
    /// interlaced (fields, as broadcast TV often is), and its height.
    pub interlaced: Option<bool>,
    pub height: Option<u32>,
    /// Byte offsets (packet starts) in the sample: the first keyframe's parameter sets (where a
    /// decoder can begin), and the first PAT and PMT (what a demuxer needs before anything).
    pub keyframe_at: Option<usize>,
    pub pat_at: Option<usize>,
    pub pmt_at: Option<usize>,
}

impl Sniff {
    pub fn of(bytes: &[u8]) -> Self {
        let container = container(bytes);
        let mut found = Self {
            container,
            ..Self::default()
        };
        if container == "mpeg_ts" {
            let program = program_map(bytes);
            found.video = program.video.map(|v| v.1);
            found.audio = program.audio;
            found.pmt_at = program.at;
            found.pat_at = bytes
                .as_chunks::<188>()
                .0
                .iter()
                .position(|p| pid(p) == 0 && unit_start(p).is_some())
                .map(|i| i * 188);
            if let Some((pid, codec)) = program.video
                && let Some((at, sps)) = first_keyframe(bytes, pid, codec)
            {
                found.keyframe_at = Some(at);
                if let Some(sps) = sps {
                    found.interlaced = Some(sps.interlaced);
                    found.height = Some(sps.height);
                }
            }
        }
        found
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

/// The video (its packet id and codec) and audio codec a transport stream's first program map
/// announces.
#[derive(Default)]
struct Program {
    video: Option<(u16, &'static str)>,
    audio: Option<&'static str>,
    /// Byte offset of the packet the map came in.
    at: Option<usize>,
}

fn program_map(bytes: &[u8]) -> Program {
    let mut found = Program::default();
    for (n, packet) in bytes.as_chunks::<188>().0.iter().enumerate() {
        // A section starts in this packet (payload-unit-start), and it has a payload.
        let Some(payload) = unit_start(packet) else {
            continue;
        };
        let Some(&pointer) = payload.first() else {
            continue;
        };
        let table = payload.get(1 + usize::from(pointer)..).unwrap_or_default();
        if table.len() < 12 || table[0] != 0x02 {
            continue; // not a program map
        }
        let length = (usize::from(table[1] & 0x0f) << 8) | usize::from(table[2]);
        let end = (3 + length).saturating_sub(4).min(table.len());
        let info = (usize::from(table[10] & 0x0f) << 8) | usize::from(table[11]);
        let mut offset = 12 + info;
        while offset + 5 <= end {
            let pid = (u16::from(table[offset + 1] & 0x1f) << 8) | u16::from(table[offset + 2]);
            let es_info =
                (usize::from(table[offset + 3] & 0x0f) << 8) | usize::from(table[offset + 4]);
            let descriptors = &table[(offset + 5).min(end)..(offset + 5 + es_info).min(end)];
            let video = |codec| Some((pid, codec));
            match table[offset] {
                0x1b => found.video = found.video.or(video("h264")),
                0x24 => found.video = found.video.or(video("hevc")),
                0x01 | 0x02 => found.video = found.video.or(video("mpeg2video")),
                0x0f => found.audio = found.audio.or(Some("aac")),
                // AAC in LATM framing (DVB broadcasts): browsers don't decode it.
                0x11 => found.audio = found.audio.or(Some("aac_latm")),
                0x03 | 0x04 => found.audio = found.audio.or(Some("mp2")),
                0x81 => found.audio = found.audio.or(Some("ac3")),
                0x87 => found.audio = found.audio.or(Some("eac3")),
                // Private data: DVB says what it is in a descriptor (AC-3 0x6a, E-AC-3 0x7a).
                0x06 => {
                    if has_descriptor(descriptors, 0x7a) {
                        found.audio = found.audio.or(Some("eac3"));
                    } else if has_descriptor(descriptors, 0x6a) {
                        found.audio = found.audio.or(Some("ac3"));
                    }
                }
                _ => {}
            }
            offset += 5 + es_info;
        }
        if found.video.is_some() || found.audio.is_some() {
            found.at = Some(n * 188);
            break;
        }
    }
    found
}

/// A packet's payload, if a unit (section or PES packet) starts in it.
fn unit_start(packet: &[u8; 188]) -> Option<&[u8]> {
    if packet[0] != 0x47 || packet[1] & 0x40 == 0 || packet[3] & 0x10 == 0 {
        return None;
    }
    let skip = if packet[3] & 0x20 != 0 {
        5 + usize::from(packet[4])
    } else {
        4
    };
    packet.get(skip..)
}

fn pid(packet: &[u8; 188]) -> u16 {
    (u16::from(packet[1] & 0x1f) << 8) | u16::from(packet[2])
}

struct Sps {
    interlaced: bool,
    height: u32,
}

/// Where a decoder can begin on `video_pid`: the first packet whose access unit carries the
/// codec's parameter sets (H.264 SPS, HEVC VPS/SPS, MPEG-2 sequence header), with the H.264 SPS
/// read. Providers' segments don't always start there. O(n) in `bytes`.
fn first_keyframe(bytes: &[u8], video_pid: u16, codec: &str) -> Option<(usize, Option<Sps>)> {
    let packets = bytes.as_chunks::<188>().0;
    let starts = packets
        .iter()
        .enumerate()
        .filter(|(_, p)| pid(p) == video_pid && unit_start(p).is_some());
    for (index, packet) in starts {
        // The access unit's first bytes: past the PES header, then the packets after it on the
        // same stream, up to 1 KiB (where encoders put parameter sets) or the next unit.
        let pes = unit_start(packet)?;
        let mut es = pes.get(9 + usize::from(*pes.get(8)?)..)?.to_vec();
        for next in packets[index + 1..].iter().filter(|p| pid(p) == video_pid) {
            if es.len() >= 1024 || next[1] & 0x40 != 0 {
                break;
            }
            if next[3] & 0x10 != 0 {
                let skip = if next[3] & 0x20 != 0 {
                    5 + usize::from(next[4])
                } else {
                    4
                };
                es.extend_from_slice(next.get(skip..).unwrap_or_default());
            }
        }
        let header = |w: &[u8]| match codec {
            "h264" => w[3] & 0x1f == 7,
            "hevc" => matches!((w[3] >> 1) & 0x3f, 32 | 33),
            _ => w[3] == 0xb3,
        };
        let Some(at) = es.windows(4).position(|w| w[..3] == [0, 0, 1] && header(w)) else {
            continue;
        };
        let sps = (codec == "h264")
            .then(|| {
                let from = at + 4;
                let end = es[from..]
                    .windows(3)
                    .position(|w| w == [0, 0, 1])
                    .map_or(es.len(), |e| from + e);
                parse_sps(&es[from..end])
            })
            .flatten();
        return Some((index * 188, sps));
    }
    None
}

/// The few fields of an H.264 SPS (after its NAL header) needed here, per ITU-T H.264 7.3.2.1.
fn parse_sps(raw: &[u8]) -> Option<Sps> {
    // Remove emulation prevention: 00 00 03 → 00 00.
    let mut rbsp = Vec::with_capacity(raw.len());
    let mut zeros = 0;
    for &b in raw {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        rbsp.push(b);
    }
    let mut r = Bits { data: &rbsp, at: 0 };
    let profile = r.bits(8)?;
    r.bits(16)?; // constraint flags, level
    r.ue()?; // seq_parameter_set_id
    let mut chroma = 1;
    if matches!(
        profile,
        100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138 | 139 | 134 | 135
    ) {
        chroma = r.ue()?;
        if chroma == 3 {
            r.bits(1)?; // separate_colour_plane_flag
        }
        r.ue()?; // bit_depth_luma
        r.ue()?; // bit_depth_chroma
        r.bits(1)?; // qpprime_y_zero_transform_bypass_flag
        if r.bits(1)? == 1 {
            for list in 0..if chroma == 3 { 12 } else { 8 } {
                if r.bits(1)? == 1 {
                    let size = if list < 6 { 16 } else { 64 };
                    let (mut last, mut next) = (8_i64, 8_i64);
                    for _ in 0..size {
                        if next != 0 {
                            next = (last + r.se()? + 256) % 256;
                        }
                        if next != 0 {
                            last = next;
                        }
                    }
                }
            }
        }
    }
    r.ue()?; // log2_max_frame_num_minus4
    match r.ue()? {
        0 => {
            r.ue()?;
        }
        1 => {
            r.bits(1)?;
            r.se()?;
            r.se()?;
            for _ in 0..r.ue()? {
                r.se()?;
            }
        }
        _ => {}
    }
    r.ue()?; // max_num_ref_frames
    r.bits(1)?; // gaps_in_frame_num_value_allowed_flag
    r.ue()?; // pic_width_in_mbs_minus1
    let map_units = r.ue()? + 1;
    let frame_mbs_only = r.bits(1)? == 1;
    if !frame_mbs_only {
        r.bits(1)?; // mb_adaptive_frame_field_flag
    }
    r.bits(1)?; // direct_8x8_inference_flag
    let rows = map_units * if frame_mbs_only { 1 } else { 2 };
    let mut height = rows * 16;
    if r.bits(1)? == 1 {
        // Frame cropping, in chroma rows (×2 again for fields, for 4:2:0).
        let (_left, _right, top, bottom) = (r.ue()?, r.ue()?, r.ue()?, r.ue()?);
        let unit = if chroma == 1 { 2 } else { 1 } * if frame_mbs_only { 1 } else { 2 };
        height = height.saturating_sub((top + bottom) * unit);
    }
    Some(Sps {
        interlaced: !frame_mbs_only,
        height,
    })
}

/// A big-endian bit reader with H.264's Exp-Golomb codes.
struct Bits<'a> {
    data: &'a [u8],
    at: usize,
}

impl Bits<'_> {
    fn bit(&mut self) -> Option<u32> {
        let byte = *self.data.get(self.at / 8)?;
        let bit = (byte >> (7 - self.at % 8)) & 1;
        self.at += 1;
        Some(u32::from(bit))
    }

    fn bits(&mut self, n: u32) -> Option<u32> {
        (0..n).try_fold(0, |v, _| Some((v << 1) | self.bit()?))
    }

    fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some((1 << zeros) - 1 + self.bits(zeros)?)
    }

    fn se(&mut self) -> Option<i64> {
        let k = i64::from(self.ue()?);
        Some(if k % 2 == 1 { (k + 1) / 2 } else { -(k / 2) })
    }
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
    fn a_real_stream_shows_where_to_begin() {
        let ts = include_bytes!("../../proxy/fixtures/h264_ac3.ts");
        let s = Sniff::of(ts);
        assert_eq!((s.video, s.audio), (Some("h264"), Some("ac3")));
        assert_eq!(s.interlaced, Some(false));
        let (key, pat, pmt) = (s.keyframe_at.unwrap(), s.pat_at.unwrap(), s.pmt_at.unwrap());
        assert!(pat < key && pmt < key, "{pat} {pmt} {key}");
        // Cut just past that keyframe: the next one is found further on, or none if the clip
        // has no other, but never one before the cut.
        let cut = &ts[key + 188..];
        if let Some(next) = Sniff::of(cut).keyframe_at {
            assert!(next > 0);
        }
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
            ..Sniff::default()
        }
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn sequence_headers_tell_interlaced_from_progressive() {
        // From x264 (high/main/baseline profiles), past the NAL header byte.
        for (sps, interlaced, height) in [
            (
                "4d4028f403c0227ef011000003000100000300321f162ea0",
                true,
                1080,
            ),
            ("42c01fda014016ec0440000003004000000c83c60ca8", false, 720),
            ("4d401eeca05a126c0440000003004000000c87c50a6580", true, 576),
            (
                "640028acd94078044fde0220000003002000000643e2c5b2c0",
                true,
                1080,
            ),
            (
                "640028acd940780227e5c044000003000400000300c83c60c658",
                false,
                1080,
            ),
        ] {
            let found = parse_sps(&hex(sps)).unwrap();
            assert_eq!(
                (found.interlaced, found.height),
                (interlaced, height),
                "{sps}"
            );
        }
        assert!(parse_sps(&[0x64]).is_none());
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
