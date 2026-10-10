Download your app below, then open it. Everything is included.

- **Windows:** `RIPTV-Windows.exe`
- **Linux:** `RIPTV-Linux` — allow the file to run first if asked.
- **Mac:** choose Apple Silicon or Intel, unzip, then open `RIPTV.app`.

Your browser opens automatically. No Rust or FFmpeg setup.

## What improved

| Since 0.2.6 | Before | Now |
|---|---|---|
| Windows download | 28.4 MB | 7.5 MB |
| Linux download | 27.0 MB | 8.1 MB |
| Mac download | 10–11 MB | 5.5–6.1 MB |
| Movie start, slow provider (500 ms per request) | 3.2–3.7 s | 1.7–2.1 s |
| Movie seek, slow provider | 2.1 s | 1.5 s |
| A live stream that ends early | 15 s to start | under 1 s |
| License | none stated | MIT, free for everyone |

Movies and episodes now start with half as many requests to your provider, and RIPTV no longer
ships a second media program. Every release's numbers: [What changed, in numbers](https://github.com/MoYusuf1/riptv/blob/main/docs/changes.md).

Apps are unsigned, so your computer may show a security warning.
The checksums and FFmpeg source download are for verification, not for running the app.

[Help](https://github.com/MoYusuf1/riptv/blob/main/docs/downloads.md)
