# What changed, in numbers

Every release records what it measurably improved, newest first. The same table opens that
release's notes (`docs/release-notes.md`), and the published release also lists its download sizes.
How each number was measured is in brackets; live-channel start times are in
[live-playback.md](live-playback.md).

## Unreleased

Live TV on a difficult real Xtream provider (10 s HLS segments, a redirected playlist link that goes
stale about every 2 minutes, 407/509 answers, connection blips), desktop Chrome, `--logs` trace:

| What | 0.2.9 | Now |
|---|---|---|
| "Provider interrupted" alarms, BBC News, 100 s | 8 (every 10 s segment gap) | 0 |
| Buffer ahead of the picture, 10 s segments | swings down to 2 s | 22–33 s (Smooth: 30–33 s) |
| Unbroken playback, BBC News | about 2 minutes, then a reconnect | 13+ minutes |
| Unbroken playback, Sky Sports Football, Smooth | not measured | 60 minutes, 0 reconnects (3 dead links renewed, 23 connection blips retried; buffer median 31 s) |
| A provider "wait 5 s" (509) | the stream ended | waited out from the buffer |
| A link that dies for good | the stream ended; ~3 s blackout and restart | reopened by the server; the stream continues |
| An update reaching the browser | could run the old app from the HTTP cache | `index.html` sent `no-cache` |

## 0.2.9

| What | 0.2.8 | 0.2.9 |
|---|---|---|
| Live channel with no sound track, converted (mock 576i interlaced) | never played; 8 provider requests in 2 s, then gave up | first picture in 0.77 s, 1 request |
| Same, 150 ms provider delay | never played | 1.02 s |
| Interlaced channel with sound (576i, deinterlaced on NVENC) | 1.03 s | 1.03 s (unchanged; the deinterlace and encode add about 0.5 s) |
| Episode name when the title only numbers it ("Show - S01E02") or is empty | the numbering, or "2. " | the episode's own name, or "Episode 2" |
| Episode plot sent as `overview`, date as `release_date`, picture as a TMDB path, `info` as a JSON string | missing | shown |
| Season keyed both "02" and "2" | two "Season 2"s, one unreachable | one season, episodes in order |
| Episode with no picture | empty box | the series' art, dimmed |
| Layout shift / frames over 50 ms while browsing (30,000-title mock) | 0 / none | 0 / none (motion added without either) |
| Thin series (mock "EN - Martha Bakes": numbered titles, nothing else), episodes with picture, name, date and summary | 0 of 6 | 6 of 6, from TVmaze |
| …time to episodes / to TVmaze's details | 67 ms / — | 67 ms / 848 ms (the page doesn't wait) |
| Cast photos for series | names only | portraits, when TVmaze has them |
| Cast photos for movies (mock movie with 4 real actors) | names only | 4 of 4, first in 220 ms (Wikipedia, looked up in parallel) |
| Memory of one conversion, 1080p 10-bit HEVC film, NVENC [private memory] | 520 MB | 419 MB |
| …same on the processor (x264), the fallback without a GPU | 966 MB | 462 MB, same speed |
| …x264 from a 4K source | 1,583 MB | 750 MB, same speed (5.4x real time) |
| RIPTV server's own memory while streaming | 7–8 MB | 7–8 MB |
| Server program (Linux, before the bundled files) | 3.41 MB | 2.66 MB (TLS on ring instead of the aws-lc C library) |
| Downloads (Windows / Linux / Mac Apple Silicon / Mac Intel) | 7.5 / 8.1 / 5.5 / 6.1 MB | 7.1 / 7.3 / 5.3 / 5.8 MB |
| Cast members linked to IMDb (mock movie with 4 real actors) | none | 4 of 4 to their exact profile (Wikidata); others to IMDb's search |
| Browser main-thread work, movie playing with controls hidden (Chrome, 10 s) | 64 ms/s | 26 ms/s |
| …style recalculations / seek-bar redraws | 60/s / 38/s | 12/s / 1.4/s (bar animates only while shown) |
| …browsing, live, paused | 0–4 ms/s | 0–4 ms/s |
| Converting progressive 1080p (yadif skipped when the MP4 index or live sniff says progressive), ffmpeg CPU for 20 s | 6.0 s | 3.9 s (−36%) |
| …same, whole movie scenario end to end (sum of 1 s ffmpeg samples) | 830% | 500% |
| Experimental player stalling on a real movie (A Carol for Two) | stuck at 72% for good | standard player takes over after 8 s; playing at 15.2 s |
| Movie and episode start | a ring with a dot | a ring with the real percentage, sweeping to 100% as the picture starts |
| Seeking in a converted movie (300 ms provider) | spinner, or a false 100% | counts up from 8%; picture and 100% at 1.25–1.65 s |
| Root text size at 1440 / 2000 px wide | 16 / 16 px | 16.2 / 19 px (the whole UI scales with the window) |
| Player controls (icons) | 1.25rem | 2rem, with 3.4rem buttons |
| Back button, title page vs player | 2rem dark vs 2.9rem light, different places | the same 55 px button in the same place |
| Account button on a title page | 41×34 px oval | 55 px circle in the profile's colour, mirroring the back button |
| Section names | Live TV · Movies · TV Shows | Live · Movies · Shows |

## 0.2.8

Mac numbers are from the release pipeline's Apple Silicon machine (Chrome decodes HEVC there);
the rest from `e2e/play.mjs` and the mock provider on Linux.

| What | 0.2.7 | 0.2.8 |
|---|---|---|
| 10-bit HEVC movie, first picture (Mac) | 634 ms, converted | 324 ms, copied |
| 10-bit HEVC movie, after a seek (Mac) | 370 ms | 292 ms |
| Hardware conversion | NVIDIA only | NVIDIA, Apple VideoToolbox (seen working on the Mac), Windows Media Foundation |
| Live HEVC check | 8-bit 1080p | 10-bit 4K, what 4K channels are |
| Offline channel (provider says 404) | 4 requests, "keeps dropping" | 1 request, "This channel is offline" in 15 ms |
| Stream info after pressing it | empty for up to 1 s | filled in 150 ms |
| Account menu | stayed open over every page | closes on any outside click or Escape |
| Menus | see-through | solid |
| Playback checks before a release | 12, Linux | 14, Linux and Mac |

## 0.2.7

Sizes from a CI build of every platform (all checks passed, including playback in Chrome).
Start times from `e2e/play.mjs`, pressing Play to moving picture, against the mock provider with
`MOCK_LATENCY_MS` added to every request; "HEVC film" is a 1080p 10-bit HEVC file converted on NVENC.

| What | 0.2.6 | 0.2.7 |
|---|---|---|
| Windows download | 28.4 MB | 7.5 MB |
| Linux download | 27.0 MB | 8.1 MB |
| Mac download (Apple Silicon) | 10.3 MB | 5.5 MB |
| Mac download (Intel) | 11.4 MB | 6.1 MB |
| …of which: dropping ffprobe (Windows / Linux) | 28.4 / 27.0 MB | 17.1 / 17.3 MB |
| …then: compressed resources, stripped server (Windows / Linux) | 17.1 / 17.3 MB | 7.5 / 8.1 MB |
| Launch to ready, Linux [unpacking included] | 37 ms | 148 ms |
| Page ready in Chrome | — | 75 ms (1.8 MB wasm loads in 5 ms; 3 MB JS heap) |
| Programs bundled | ffmpeg + ffprobe | ffmpeg only |
| Requests before a converted film starts | 6 | 3 |
| H.264 MKV with AC-3 (copied), first picture, 150 ms delay | 1.06 s | 0.60 s |
| HEVC film, first picture, 150 ms delay | 1.56 s | 1.05 s |
| HEVC film, after a seek, 150 ms delay | 0.91 s | 0.76 s |
| H.264 MKV with AC-3, first picture, 500 ms delay | 3.16 s | 1.66 s |
| HEVC film, first picture, 500 ms delay | 3.66 s | 2.11 s |
| HEVC film, after a seek, 500 ms delay | 2.07 s | 1.51 s |
| Live channel that ends early (mock 4 s clip), first picture | 15.3 s | 0.5 s (H.264), 1.0 s (HEVC converted) |
| License | none stated | MIT |

## 0.2.6

| What | Before | After |
|---|---|---|
| App | 0.2.5 | unchanged |
| README | technical | plain-language overview, links rstreamkit |

## 0.2.5

| What | 0.2.4 | 0.2.5 |
|---|---|---|
| Windows download | 113 MB | 28.4 MB |
| Linux download | 104 MB | 27.0 MB |
| Mac download (Apple Silicon) | 34.0 MB | 10.3 MB |
| Mac download (Intel) | 38.1 MB | 11.4 MB |
| Bundled ffmpeg (Linux, one binary) | 46.1 MB | 9.9 MB |
| 4K HEVC conversion, NVIDIA on Linux [10 s clip, CPU time] | 27.7 s (about 9 cores, no NVENC) | 3.5 s (about 1.6 cores, NVENC) |
| Release checks | build, smoke test, unit tests | plus playback in Chrome through the packaged app |

## 0.2.4

| What | 0.2.3 | 0.2.4 |
|---|---|---|
| Seek bar after a drag that ends where it began | froze | works |
| Controls under the pointer | could fade mid-drag | stay until you're done |

## 0.2.3

| What | 0.2.2 | 0.2.3 |
|---|---|---|
| Sound vs picture after resuming a converted title [first audio frame; stream ends] | up to ~10 s apart | under 0.1 s |
| Quitting the app | system monitor | Quit in the menu, stops within 2 s |
