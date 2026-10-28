# RIPTV

**A fast, private TV app for your IPTV service.** Live TV, movies and shows, in your browser.

## Why RIPTV

- **Channels usually start in 1–3 seconds**, with sound. That includes 4K and surround-sound channels
  most browsers can't play.
- **Private.** It runs on your own computer. No account, ads or tracking. Your logins never leave
  your machine except to reach your own provider.
- **Free and open source**, under the MIT License.
- **Small and simple.** A single download of 10–28 MB. Nothing else to install.
- **Picks up where you left off.** Resume movies and episodes, keep a list of favourites, and have a
  separate profile for each person.
- **Uses your graphics card** where it can, to keep 4K smooth and your computer quiet.

## Download

[Windows](https://github.com/MoYusuf1/riptv/releases/latest/download/RIPTV-Windows.exe) ·
[Linux](https://github.com/MoYusuf1/riptv/releases/latest/download/RIPTV-Linux) ·
[Mac (Apple Silicon)](https://github.com/MoYusuf1/riptv/releases/latest/download/RIPTV-Mac-AppleSilicon.zip) ·
[Mac (Intel)](https://github.com/MoYusuf1/riptv/releases/latest/download/RIPTV-Mac-Intel.zip)

1. Open the download. On a Mac, unzip it first and open **RIPTV.app**.
2. Your browser opens RIPTV.
3. Pick **Public TV** to try free channels, or add your own service.

It works with **Xtream Codes** logins and **M3U / M3U8** playlists. RIPTV tells you when an update is
ready. To close it, choose **Quit** in the account menu.

The apps aren't signed yet, so your computer may show a security warning the first time.
[Download help](docs/downloads.md)

## Under the hood

RIPTV is written in Rust. Most channels play through your browser's own video player. A small
built-in converter steps in only when a channel uses a format your browser can't play.

There is also an **Experimental player** (in the account menu) built on
**[rstreamkit](https://github.com/MoYusuf1/rstreamkit)**. rstreamkit is a video engine written from
scratch in Rust that unpacks streams and decodes surround sound right inside the browser, with no
converter at all. It's still being tested.

[Build from source](docs/setup.md) · [How it works](docs/reference.md) · [Roadmap](docs/roadmap.md) ·
[For developers](docs/technical-breakdown.md) · [Bundled software](docs/third-party.md)

## Built for difficult providers

Many IPTV providers cut corners: video in long chunks, links that expire after a couple of minutes,
"slow down" answers. Phone and TV apps quietly ride over this. RIPTV does too, and every technique
below was measured on a real provider that cut out every two minutes:

| What the provider does | What RIPTV does | Before → after |
|---|---|---|
| Sends video in 10-second chunks, silent in between | Starts three chunks behind live, like TV apps, and only calls silence an outage when the buffer can't cover it | "Provider interrupted" 8 times in 100 s → none |
| Its stream link goes stale every ~2 minutes (refuses, or stops updating) | Fetches a fresh link from your login in the background and carries on | a cut every ~2 min → 13+ minutes unbroken |
| Answers "too busy, wait 5 s" | Waits it out while the buffer plays | a cut → nothing visible |
| A chunk arrives late | **Smooth** (in the player's settings) starts 45 s behind live for more room | |

When something can't be ridden out, RIPTV reconnects on its own, then tries the channel's other
format, then a backup feed you've approved. Run it with `--logs` to see each of these as it happens.

## License

RIPTV is free for everyone under the [MIT License](LICENSE): use it, change it, share it.
The bundled FFmpeg has its own license ([details](docs/third-party.md)).
