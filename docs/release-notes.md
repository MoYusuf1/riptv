Download your app below, then open it. Everything is included.

- **Windows:** `RIPTV-Windows.exe`
- **Linux:** `RIPTV-Linux` — allow the file to run first if asked.
- **Mac:** choose Apple Silicon or Intel, unzip, then open `RIPTV.app`.

Your browser opens automatically. No Rust or FFmpeg setup.

## What improved

**Live TV no longer cuts out.** Some IPTV providers are unreliable: every couple of minutes
their stream link stops working, or their server goes quiet for a while. TV-box and phone apps
quietly work around this, but RIPTV used to give up and restart the channel, which you saw as a
cut. Now RIPTV keeps about 30 seconds of video ready, and when the provider misbehaves it quietly
gets a fresh link in the background and carries on from the same moment. You just keep watching.

Measured on a real provider that used to cut out every ~2 minutes on desktop:

| Since 0.2.9 | Before | Now |
|---|---|---|
| Unbroken live TV on that provider | a cut every ~2 minutes | 0 cuts in 3.5 hours of sports across 9 channels |
| Its stream link going stale (every ~2 min) | the stream ended | a fresh link fetched in the background |
| The provider not answering | 33 s of waiting, then a cut | a fresh link after 4 s |
| A link that dies for good | a blackout and restart | the server reconnects and carries on |
| "Provider interrupted" on screen | every ~10 s | never while the picture plays |
| Buffer ahead of the picture | swinging down to 2 s | a steady ~30 s (Smooth) |
| Encrypted channels (AES-128) | failed | play |
| With a Discord-style screen share running | not measured | no cuts |

Also: an update now always reaches your browser (no stale app from the cache), keyboard focus
rings on every control, one consistent motion curve, the loading ring keeps turning with
"reduce motion" on, and a quieter no-sound badge. Every release's numbers:
[What changed, in numbers](https://github.com/MoYusuf1/riptv/blob/main/docs/changes.md).

Apps are unsigned, so your computer may show a security warning.
The checksums and FFmpeg source download are for verification, not for running the app.

[Help](https://github.com/MoYusuf1/riptv/blob/main/docs/downloads.md)
