//! Live TV playback in Rust: HLS playlists, MPEG-TS demuxing and fMP4 muxing (pure, testable
//! natively), plus the MediaSource glue that feeds a `<video>` element (wasm only).
//!
//! ponytail: mid-stream SPS changes and resolution switches are not handled; they surface as
//! playback errors rather than being papered over. Timestamp jumps are.

pub mod avc;
pub mod fmp4;
pub mod hls;
#[cfg(target_arch = "wasm32")]
pub mod mse;
pub mod ts;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Ts(#[from] ts::Error),
    #[error("bad playlist: {0}")]
    Playlist(String),
    #[error("{0}")]
    Unsupported(String),
}

/// What to give `MediaSource.addSourceBuffer` and then append first.
pub struct Init {
    pub bytes: Vec<u8>,
    pub mime: String,
}

pub struct Output {
    /// Present for the first segment only.
    pub init: Option<Init>,
    /// moof+mdat to append; empty if the segment held no samples.
    pub fragment: Vec<u8>,
    pub skipped_audio: Option<String>,
}

const WRAP: u64 = 1 << 33; // PES timestamps are 33 bits
/// A segment starting further than this (90 kHz ticks, 2 s) from where the last one ended is a
/// new timeline (stream restart, ad splice, missing tag) and gets glued on instead of leaving a gap.
const JUMP: u64 = 2 * 90_000;

/// Turns consecutive HLS TS segments into a continuous fMP4 stream.
#[derive(Default)]
pub struct Transmuxer {
    base: Option<u64>,
    /// Last (unwrapped) timestamp seen, used to resolve the 33-bit rollover.
    last: u64,
    /// Where the previous fragment ended, relative to `base`, in 90 kHz ticks.
    end: u64,
    seq: u32,
    sent_init: bool,
    has_audio: bool,
    audio_next: Option<u64>,
}

impl Transmuxer {
    /// Picks the representation of a 33-bit timestamp nearest to the last one seen.
    fn unwrap(&mut self, t: u64) -> u64 {
        let mut u = t + self.last / WRAP * WRAP;
        if u + WRAP / 2 < self.last {
            u += WRAP;
        } else if u > self.last + WRAP / 2 && u >= WRAP {
            u -= WRAP;
        }
        self.last = self.last.max(u);
        u
    }

    pub fn push(&mut self, segment: &[u8]) -> Result<Output, Error> {
        let d = ts::demux(segment)?;

        let init = if self.sent_init {
            None
        } else {
            let sps = d
                .sps
                .as_deref()
                .filter(|s| s.len() >= 4)
                .ok_or(ts::Error::NoVideoParams)?;
            let pps = d.pps.as_deref().ok_or(ts::Error::NoVideoParams)?;
            let (width, height) = avc::dimensions(sps)
                .ok_or_else(|| ts::Error::BadSps("cannot read the picture size".into()))?;
            self.has_audio = d.aac.is_some();
            let audio = d
                .aac
                .map(|a| format!(",mp4a.40.{}", a.object_type))
                .unwrap_or_default();
            self.sent_init = true;
            Some(Init {
                bytes: fmp4::init_segment(
                    &fmp4::VideoParams {
                        sps,
                        pps,
                        width,
                        height,
                        pixel_aspect: avc::pixel_aspect(sps).unwrap_or((1, 1)),
                    },
                    d.aac.as_ref(),
                ),
                mime: format!(
                    "video/mp4; codecs=\"avc1.{:02x}{:02x}{:02x}{audio}\"",
                    sps[1], sps[2], sps[3]
                ),
            })
        };

        let video: Vec<(u64, u64)> = d
            .video
            .iter()
            .map(|s| (self.unwrap(s.dts), self.unwrap(s.pts)))
            .collect();
        let audio: Vec<u64> = d.audio.iter().map(|s| self.unwrap(s.pts)).collect();
        let first = video
            .first()
            .map(|v| v.0)
            .into_iter()
            .chain(audio.first().copied())
            .min();
        let Some(first) = first else {
            return Ok(Output {
                init,
                fragment: vec![],
                skipped_audio: d.skipped_audio,
            });
        };

        // Timelines start at zero, and a jump in the source timeline is re-based so it plays on.
        let base = match self.base {
            Some(b) if first.abs_diff(b + self.end) <= JUMP => b,
            _ => {
                self.audio_next = None;
                first.saturating_sub(self.end)
            }
        };
        self.base = Some(base);

        let mut runs = vec![];
        let mut end = 0;
        if !video.is_empty() {
            let mut last_dur = 3003; // ~29.97 fps, only used if the segment has a single frame
            let samples: Vec<_> = d
                .video
                .iter()
                .enumerate()
                .map(|(i, s)| {
                    let dur = video
                        .get(i + 1)
                        .map_or(last_dur, |n| n.0.saturating_sub(video[i].0) as u32);
                    last_dur = dur;
                    fmp4::Sample {
                        duration: dur,
                        key: s.key,
                        cts: (video[i].1 as i64 - video[i].0 as i64) as i32,
                        data: &s.data,
                    }
                })
                .collect();
            let start = video[0].0.saturating_sub(base);
            end = end.max(start + samples.iter().map(|s| s.duration as u64).sum::<u64>());
            runs.push(fmp4::TrackRun {
                track: fmp4::VIDEO_TRACK,
                base_time: start,
                samples,
            });
        }
        if let (Some(cfg), true, false) = (d.aac, self.has_audio, audio.is_empty()) {
            let rate = cfg.sample_rate() as u64;
            let derived = audio[0].saturating_sub(base) * rate / 90_000;
            // Keep audio gapless: 90 kHz -> sample-rate rounding must not open 1-tick holes between fragments.
            let start = match self.audio_next {
                Some(next) if derived.abs_diff(next) <= 1024 => next,
                _ => derived,
            };
            let stop = start + 1024 * audio.len() as u64;
            self.audio_next = Some(stop);
            end = end.max(stop * 90_000 / rate);
            let samples = d
                .audio
                .iter()
                .map(|s| fmp4::Sample {
                    duration: 1024,
                    key: true,
                    cts: 0,
                    data: &s.data,
                })
                .collect();
            runs.push(fmp4::TrackRun {
                track: fmp4::AUDIO_TRACK,
                base_time: start,
                samples,
            });
        }
        self.end = end;

        self.seq += 1;
        Ok(Output {
            init,
            fragment: fmp4::fragment(self.seq, &runs),
            skipped_audio: d.skipped_audio,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_unwrap_survives_the_33_bit_rollover() {
        let mut t = Transmuxer::default();
        let near_end = WRAP - 90_000;
        assert_eq!(t.unwrap(near_end), near_end);
        // 0.5 s later the counter wrapped to a small number; we must keep counting up.
        assert_eq!(t.unwrap(45_000 - 1), WRAP + 45_000 - 1);
        assert_eq!(
            t.unwrap(near_end + 1000),
            near_end + 1000,
            "a slightly older timestamp stays put"
        );
    }
}
