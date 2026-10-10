# What changed, in numbers

Every release records what it measurably improved, newest first. The same table opens that
release's notes (`docs/release-notes.md`), and the published release also lists its download sizes.
How each number was measured is in brackets; live-channel start times are in
[live-playback.md](live-playback.md).

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
