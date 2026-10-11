# rstreamkit: a movie stalls at the start, buffered but never decoded

**For:** the rstreamkit engine (`https://github.com/MoYusuf1/rstreamkit`), used by RIPTV as its
Experimental player. **From:** RIPTV, 2026-10-11. RIPTV tracking issue: MoYusuf1/riptv#6.
Everything below is measured; nothing is estimated unless it says so.

## Summary

With the Experimental player on, the movie **A Carol for Two** (2024, 1h 24m, 1920×1080) from a
real Xtream provider loads **1.8 s** of media into MediaSource, then sits at `readyState` 1
(HAVE_METADATA) for **293 s** until the viewer leaves: no frame decoded, **6,156 bytes** of sound
decoded, no error, no `Status` reported to the app, and **no further data fetched** although the
element reports `networkState` 2 (LOADING) the whole time. The standard player (ffmpeg) plays the
same title. RIPTV now works around it (see *What RIPTV does now*), but the cause is inside the
engine and isn't reproducible with any file built locally (see *What was tried*).

## Environment

| | |
|---|---|
| rstreamkit | `72f8cddebcbc0cd6705888c740b792a68dc06ca3` (0.1.0, default features `sound`, `vod`) |
| RIPTV | 0.2.8 + main at `078e044` (the stall was recorded before that commit's workaround) |
| Browser | Chrome 155.0.0.0, Linux x86_64 (`Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/155.0.0.0 Safari/537.36`) |
| Machine | 16-core x86_64, NVIDIA RTX 2060 SUPER (driver 615.78.08), CachyOS Linux 7.2 |
| Source | a real Xtream provider's VOD file, fetched through RIPTV's local proxy (`/proxy?url=…`, byte ranges passed through) |
| Entry points | `rstreamkit::mse::probe` → `Movie::verdict(&mse::can_play)` = `Verdict::Rust` → `mse::play_movie(video, movie, url, fetch, start = 0.0, report)` |

What the verdict tells us about the file, from `vod.rs` at this revision: `Verdict::Rust` means it
was *not* playable natively, its video has an `avcC` (H.264), it was not flagged interlaced, and
its sound is decodable by the engine (AC-3, E-AC-3 or MP2) or is AAC in Matroska. The provider's
own metadata for this title gives no codec or container details.

## The trace

RIPTV's `--logs` trace for the session (`[8d59f391]`), every line. Times are UTC; `t_ms` is from
pressing Play. `ahead_s` is `video.buffered` end minus `currentTime` (`video.buffered` is the
intersection of all SourceBuffers' ranges), `ready` is `readyState`, `network` is `networkState`,
`audio_bytes` is `webkitAudioDecodedByteCount`.

```
02:35:19.326Z open      experimental=true kind="movie" resume_s=0 t_ms=0 title="A Carol for Two"
02:35:20.865Z engine    engine="rust" t_ms=1564
02:35:22.180Z play      ahead_s=1.8 at_s=0.0 network=2 paused=false ready=1 t_ms=2880
02:35:30.866Z stuck     ahead_s=1.8 at_s=0.0 ms=8685 network=2 paused=false ready=1 starting=true t_ms=11566
02:35:50.866Z health    ahead_s=1.8 at_s=0.0 audio_bytes=6156 dropped=0 fps=0.0 height=1080 width=1920 hidden=false network=2 paused=false ready=1 t_ms=31567
02:36:21.850Z health    ahead_s=1.8 at_s=0.0 audio_bytes=6156 dropped=0 fps=0.0 height=1080 width=1920 hidden=true  network=2 paused=false ready=1 t_ms=62547
02:36:51.849Z health    ahead_s=1.8 at_s=0.0 audio_bytes=6156 dropped=0 fps=0.0 height=1080 width=1920 hidden=true  network=2 paused=false ready=1 t_ms=92548
02:39:26.851Z health    ahead_s=1.8 at_s=0.0 audio_bytes=6156 dropped=0 fps=0.0 height=1080 width=1920 hidden=true  network=2 paused=false ready=1 t_ms=247548
02:40:12.353Z closed    ahead_s=1.8 at_s=0.0 network=2 paused=false ready=1 stalled_ms=0 stalls=0 t_ms=293046
```

## What the numbers say

| Measurement | Value | Reading |
|---|---|---|
| Probe (Play pressed → verdict) | **1,564 ms** | Several range requests to the provider; the same step takes 19–25 ms against a local server. Normal for a remote provider; the probe itself succeeded. |
| Verdict → `play()` called with data | **1,316 ms** (t=1564 → 2880) | The first media data arrived and was appended. |
| Media buffered | **1.8 s**, from position 0 | Both SourceBuffers hold data covering 0–1.8 s (the intersection is non-empty from `currentTime` 0). |
| After that | **0 s** more in 290 s | The engine stopped fetching after its first 1.8 s. |
| `readyState` | **1** for 293 s | Metadata (the init segment) accepted; **no frame decodable at 0**. |
| Frames decoded / dropped | **0 / 0** (fps 0.0) | The video decoder never produced a frame. |
| Sound decoded by the browser | **6,156 bytes**, never more | The engine's sound reached the browser's decoder briefly. As 48 kHz 16-bit stereo that is about 32 ms. (The format the engine outputs, FLAC in fMP4 per RIPTV's notes, makes the exact duration uncertain.) |
| Errors | **none**: no `media_error`, no `failure`, no `Status::Failed` or `Unsupported` | The stall is silent. |
| Picture size reported | 1920×1080 | The init segment's track description was read correctly. |

So the init segment is fine; media covering the start is in both buffers; but nothing at
`currentTime` 0 decodes, and the engine then waits indefinitely, presumably for playback to
advance before fetching more.

## What was tried, and didn't reproduce it

Every file below was served by RIPTV's mock provider (`cargo run -p riptv --example
mock_provider` with `MOCK_MOVIE=<file>`, HTTP byte ranges) and played with the Experimental
player in Chrome 155 (headless, Linux). `startup_ms` is from RIPTV's trace (open → `playing`).

| File | Video | Sound | Result |
|---|---|---|---|
| Mock movie 4 | 384×216 H.264 High, B-frames | AC-3 5.1 | plays, 128–165 ms; seek plays in 55–73 ms |
| 3 min, resumed at 1:08 | 640×360 H.264 | AC-3 5.1 | resumes at 68.0 s, 92 ms |
| 150 s | 1920×1080 H.264, GOP 48 | **E-AC-3 5.1**, 384 kb/s | plays, 179 ms, 246,240 audio bytes in the first second |
| 150 s | 1280×720 **H.264 High 10** (yuv420p10le) | AC-3 | plays, 154 ms (Chrome decodes it) |
| 150 s | 1280×720 **H.264 High 4:4:4** (yuv444p) | AC-3 | plays, 147 ms |
| 150 s, **video starts at 0.417 s** | 1280×720 H.264 | AC-3 5.1 from 0 | plays, 190 ms (start offset handled) |
| 150 s, **sound starts at 0.5 s** | 1280×720 H.264, 3 B-frames | E-AC-3 5.1 from 0.5 s | plays, 206 ms |

None stalls. So the cause is something specific to this provider file or how it is served:
its exact track setup, its index (where the Cues are, how sparse), its first cluster, or the
provider's answers to range requests (redirects, `Content-Range` details, short reads).

## Hypotheses, most likely first

1. **The first decodable picture isn't at the start of what was appended.** E.g. the first
   cluster starts mid-GOP, or with open-GOP/leading B-frames that reference a previous keyframe,
   or the first keyframe isn't flagged as one (Matroska `SimpleBlock` keyframe bit / Cues
   pointing at a non-IDR frame). Chrome then holds at 0 with data it can't start from: exactly
   `readyState` 1 with a non-empty buffered range. Fits every number above.
2. **A sound decode problem that blocks the sound buffer.** 6,156 decoded bytes, then nothing:
   if the engine's E-AC-3/AC-3 decode or its output framing fails after the first frames (an
   unusual bitstream mode, dependent substreams, 7.1, a sample-rate change), the sound
   SourceBuffer could end short of the video, and playback at 0 waits for both. Fits the tiny
   `audio_bytes`.
3. **The fetch loop stops after the first piece and never resumes,** because it waits for
   `currentTime` to move (which needs a decoded frame) before asking for more. Not a cause on
   its own, but it turns any start problem into a permanent stall instead of a recoverable one.
4. **Something about the provider's range answers** (a redirect to a CDN, a `200` instead of
   `206`, a missing or different `Content-Length`/`Content-Range`, a short body) that leaves the
   first piece truncated mid-frame. The mock serves textbook `206`s, which would explain why no
   local file reproduces it.

## What would settle it

- **The file's track and index details.** RIPTV can capture them: run `cargo riptv --logs`, start
  the title with the Experimental player on, and while the session is open:
  `curl http://127.0.0.1:3000/diagnostics` (lists the session id) then
  `curl -X POST http://127.0.0.1:3000/diagnostics/<session>/probe`, which reports container,
  codecs and timings without exposing the address. Better still, an `ffprobe -show_streams
  -show_format` and `-show_packets -read_intervals %+5` of the first five seconds, and an
  `mkvinfo` (if Matroska) for the Cues and first cluster.
- **Engine-side instrumentation** (what RIPTV can't see from outside): for each SourceBuffer,
  the buffered ranges as start–end pairs (not just the intersection's end); the first video
  sample's DTS/PTS after the engine's `shift` and whether it is a sync sample; the byte ranges
  requested and the status, length and `Content-Range` of each answer; and why the fetch loop
  stopped after 1.8 s.
- **A `Status` for a stall.** When `play()` has data buffered at `currentTime` and no frame
  decodes for N seconds, report it (e.g. `Status::Failed("no picture decoded")`) so the app can
  act, rather than waiting forever.

## What RIPTV does now

Since `078e044`, if the Experimental player is still the engine **8 s** after a movie or episode
starts and no frame has been decoded (`loadeddata` not fired), RIPTV switches the title to the
standard player (ffmpeg conversion), the same as it does when the engine reports `Unsupported`.
The trace records `engine engine="ffmpeg" why="the experimental player stalled"`. In RIPTV's full
playback check (15 checks, including the Experimental player on a live channel and a movie with a
seek) this never fires on titles that play.

## A second finding for the engine: Matroska's interlace flag is read the wrong way

`mkv.rs` at `72f8cdd`, line 133: `0x9A => e.video.4 = uint(p) > 1,` reads `FlagInterlaced` as
"interlaced when greater than 1". In Matroska, **0 = undetermined, 1 = interlaced, 2 =
progressive**. So every progressive MKV that states it (ffmpeg writes `2`) is reported as
interlaced, and a truly interlaced one (`1`) is reported as progressive. Found by RIPTV's
Playwright check on a Mac: a 10-bit HEVC MKV was converted instead of copied. Fix: `uint(p) == 1`.
RIPTV currently ignores the flag for Matroska until this changes (`app/src/playback.rs`).
