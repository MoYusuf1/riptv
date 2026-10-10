# What changed, in numbers

Every release records what it measurably improved, newest first. The same table opens that
release's notes (`docs/release-notes.md`), and the published release also lists its download sizes.
How each number was measured is in brackets; live-channel start times are in
[live-playback.md](live-playback.md).

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
