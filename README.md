# RIPTV

RIPTV is an Xtream Codes IPTV client, all Rust and small enough to ignore: the web app is about
0.7 MB before compression, and the proxy is a 2.9 MB binary that idles at ~4 MB of RAM on two threads.

| Crate | What it is |
|---|---|
| `xtream` | API client: login, live/movie/series lists, episodes, stream URLs. Native and wasm. |
| `proxy` | Localhost pass-through proxy (browsers can't reach IPTV servers directly) that also serves the web app. |
| `app` | Dioxus web UI, compiled to WebAssembly. |

Video and sound handling is not in this repository: it is [rffmpeg](https://github.com/MoYusuf1/rffmpeg), a separate pure-Rust
library (HLS, MPEG-TS and MP4/MKV movie files in, fMP4 out, AC-3/E-AC-3/MP2 sound decoding in Rust, and the
MediaSource glue that plays it in a `<video>`). `app` pulls it from its private GitHub repo (cargo uses your git SSH
access, see `.cargo/config.toml`). To work on both at once, clone it next to this folder and build with
`--config 'patch."ssh://git@github.com/MoYusuf1/rffmpeg.git".rffmpeg.path="../rffmpeg"'`.

Everything you write is Rust. The only non-Rust file in the build output is the small JS loader that
`wasm-bindgen` generates (browsers can't start WebAssembly without one); it is never edited by hand.

## How it fits together

```text
 browser (WASM)                       your machine                      internet
 ┌───────────────────────┐  /proxy   ┌───────────────────┐            ┌────────────────┐
 │ app    screens, state │ ────────▶ │ riptv             │ ─────────▶ │ IPTV provider  │
 │ ├ xtream  API client  │  ?url=…   │ serves the app,   │  public    │ and its CDNs   │
 │ └ rffmpeg live TV     │ ◀──────── │ relays the bytes  │  addresses └────────────────┘
 └───────────────────────┘           └───────────────────┘  public hosts only
```

| Component | Job | Checked by |
|---|---|---|
| `xtream` | Speaks the Xtream Codes API and builds stream URLs, routing everything through the proxy when asked. Lists are decoded record by record straight from the response bytes: a 30,000-title list peaks at ~7 MB of heap instead of ~70 MB. | Unit tests on real-world JSON quirks |
| `rffmpeg` | Live: playlist → MPEG-TS demux → fMP4 → the browser's MediaSource. Movies and episodes: MP4 or MKV index → byte ranges → fMP4, sound decoded on the way. Only `mse` touches the browser; the rest is plain Rust that runs and tests natively. | Real HLS segments, MP4 and MKV files, decoded by `ffmpeg` |
| `proxy` (`riptv`) | The one native program. Serves the built app and relays requests for the app alone, to public addresses only, with `Range`, redirect checks and a strict CSP. | End-to-end tests through a real proxy |
| `app` | Screens and state only, with no protocol or media code. | Driven in a browser |

Where a change belongs: protocol quirks in `xtream`, anything about video bytes in `rffmpeg`, anything
at the network boundary in `proxy`, and `app` only arranges them.

## Run

```sh
rm -rf target/dx && dx build --web --release -p app  # once, and after UI changes (dx never deletes old builds)
cargo riptv                                          # then open http://127.0.0.1:3000 and sign in
```

That's all: there is no host list to keep. The proxy relays only for the app itself (not for other
websites), and only to public addresses, so a provider, or a redirect it sends, can't point it at your
router or other machines. A server on your own network (or this machine) works because signing in
approves that one address. `cargo riptv` is an alias for `cargo run --release -p riptv --` (see
`.cargo/config.toml`); `IPTV_PORT` changes the port.

Open it in a normal browser (Chrome, Firefox, Safari). Sound and real fullscreen depend on the browser,
and an embedded preview pane may have neither; the player falls back to filling the page. On the live
player Space plays or pauses, F is fullscreen, M mutes, and double-click is fullscreen.

## Try it without a provider

```sh
cargo run -p riptv --example mock_provider      # fake provider on :8081, login demo / demo
cargo riptv
```

Open the page and click "Try the demo". It has movies with AC-3 sound as an MP4 and as an MKV (served
with byte ranges, like a real provider, and played by rffmpeg), one with DTS sound that needs ffmpeg, a
two-season series whose episodes are the same two files, an HLS channel, a simulated live channel with
a sliding playlist window, and an anamorphic PAL channel (720x576 with 64:45 pixels) that must come out
16:9. Every channel has a generated day-long guide. `MOCK_MOVIE=/path/film.mkv cargo run -p riptv
--example mock_provider` adds your own file as a sixth movie, which is the best way to try seeking.

## Using it

The three sections (Live TV, Movies, Series) live on a floating rail, which becomes a tab bar at the
bottom on a phone; there the category list turns into a **Categories** button.

Live TV shows every channel as a tile grid until you pick a category; then it becomes a channel list
next to the player, with the channel's schedule as a timeline underneath. Every list can be sorted
(newest or oldest added, A to Z, Z to A, top rated). Movies and Series are poster
grids. Each section is loaded once, so category counts are exact and switching category is instant; the
account menu's **Refresh** reloads it. Live channels are endless streams, so they can't be downloaded.

### Movie and series pages

Opening a title gives a full-page view under the floating chrome: a big backdrop, the title and
genres, **Play** (or **Resume**), the trailer and download buttons, the year, length, age rating and
quality, the rating, the director and the plot, and a card of facts (including when a movie would
end if you started now). Below are the cast, the trailer and, for a series, season tabs and an
episode grid with thumbnails, lengths and summaries. Everything shown is what the provider's panel
knows; what it doesn't send is left out. (Panels send cast as names only, so the faces are initials.)

### The player

Movies and episodes play full page: back and title on top, a seek bar that shows what is buffered,
play/pause, 10-second skips, volume, time, playback speed, picture-in-picture and fullscreen. The
controls fade while you watch. Keys: Space or K plays and pauses, J/← and L/→ skip 10 seconds, ↑/↓
change the volume, M mutes, F is fullscreen, N is the next episode, Esc goes back. Where you stopped
is remembered in the browser (not sent anywhere) and offered the next time, with **Start over**; when an
episode ends the next one is offered, and starts in a few seconds unless you cancel.

## Compatibility mode (ffmpeg)

Many browsers, Chrome on Linux among them, can't decode HEVC (every 4K channel), AC-3, MP2 or
AAC-Main sound, interlaced video smoothly, or raw MPEG-TS streams. When a stream is one of those the
player hands it to the proxy, which uses `ffmpeg` (if installed) to turn it into H.264 + AAC on the
fly: the video is copied untouched when it's already fine, and only HEVC or interlaced video is
re-encoded (NVENC if you have an NVIDIA card, otherwise x264), deinterlaced to full motion rate and
capped at 1080p (`RIPTV_MAX_HEIGHT=2160` for full 4K, if your browser decodes it smoothly). Without
ffmpeg those channels say why, and a sound-only problem still plays the picture. Everything else stays
in the pure-Rust player. ffmpeg follows redirects itself, so its own connections aren't held to the
public-address rule the proxy applies to the page's requests.

A converted movie or episode is one continuous stream with no index, which the browser can't seek in,
so its seek bar restarts the conversion from the chosen second (a moment's wait). Movies and episodes
are converted only when nothing in the page can play them (HEVC, DTS and TrueHD sound, interlaced
video), and that is decided before playback starts, never halfway through.

**Sound in Rust (experimental)** is a switch in the account menu. On, live channels with
AC-3, E-AC-3 or MP2 sound are decoded by rffmpeg inside the browser (5.1 is mixed down to stereo,
played as FLAC) and skip ffmpeg. It applies to the next channel you open and is remembered. Off is
the default: ffmpeg does it. It is checked against ffmpeg's own decoding in rffmpeg's tests, but has
seen less real-world use, and it adds about 257 KB to the page whether it's on or not.

Press `I` on the live player (or the info button) for the picture size, real frame rate, dropped
frames and buffer, which tells a slow stream from a slow decoder.

## What plays

**Movies and episodes** are read by the page itself, in Rust. Before anything plays, rffmpeg looks at
the file's index (MP4's `moov`, Matroska's tracks and cues: a few range requests) and decides:

| If the file is... | it is played by... |
|---|---|
| something the browser plays itself (H.264 with AAC, say) | the browser, as a plain `<video>` |
| H.264 with AC-3, E-AC-3, MP2 (or AAC where the browser can't read the container) | **rffmpeg**: the picture is copied, the sound is decoded in Rust, and both are fed to MediaSource a few seconds ahead of the playhead. Nothing is converted, nothing buffers while sound is "fixed", and a seek reads from the new place |
| HEVC, DTS, TrueHD, interlaced, or anything it can't read | the proxy's ffmpeg (above), chosen up front |

Decoded sound is stereo (5.1 is mixed down) and is held uncompressed, which is a lot of memory for a
browser that keeps only about a minute of sound in a buffer, so the page reads a few seconds at a time. A server that doesn't answer byte-range requests, or a file it can't read the
index of, is left to the browser.

**Live TV** goes through `rffmpeg`:
HLS with MPEG-TS segments carrying **H.264 video and AAC audio**. Anything else is reported, not
misplayed: HEVC, AES-128, fMP4 segments, continuous (non-HLS) `.ts` streams and AC-3/MP2 audio are not
played directly, and compatibility mode covers them (AC-3 and MP2 can instead be decoded in Rust,
see above). There is no adaptive bitrate: one variant is picked up front.

## Test

```sh
cargo test        # xtream and proxy (the wasm-only app is built with dx; rffmpeg has its own tests)
```

rffmpeg's own tests (`cargo test` in its folder) run real HLS segments through the transmuxer and have
`ffmpeg` decode the result (skipped if `ffmpeg` isn't installed).
