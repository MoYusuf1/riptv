# RIPTV

RIPTV is a local IPTV player for Xtream Codes and M3U/M3U8 playlists, written in Rust. The current production web app is about
1.3 MB before compression; the proxy is a small native binary.

## Quick start (local app)

RIPTV runs on your computer and opens in your browser; no hosting account is needed. On Linux or
macOS, install Rust/Cargo, Git, and the WebAssembly target, then install the Dioxus CLI used by this
project:

```sh
rustup target add wasm32-unknown-unknown
cargo install dioxus-cli --version 0.7.10 --locked
```

If your Rust came from a Linux distribution instead of `rustup`, install its
`wasm32-unknown-unknown` target package instead. You also need Git SSH access to the private
`rstreamkit` dependency. `ffmpeg` is strongly recommended for streams your browser cannot decode.

From a checkout of this repo, run:

```sh
./scripts/setup.sh
```

The script checks prerequisites, builds the web app, starts the local server, and tells you the
address. Open **http://127.0.0.1:3000** and sign in to your IPTV provider, or choose **Try the demo**.
Press **Ctrl+C** to stop it. After the first build, `cargo riptv` starts it again without rebuilding;
rerun `./scripts/setup.sh` after app changes. The script does not install packages or delete your
build files. To build without starting the server, use `./scripts/setup.sh --build-only`.

## Phone and Home Screen app

The web build includes an installable PWA manifest, icons, and a small service worker. On an iPhone,
open a deployed **HTTPS** RIPTV address in Safari, then use **Share → Add to Home Screen**. The
service worker keeps only the app's HTML shell and icons for a fallback when offline; it never
caches IPTV lists, credentials, or video. Watching still requires a connection.

For HTTPS HLS (`.m3u8`) on a browser that supports it, live TV first tries the device's native
video player. Other live streams use the local ffmpeg compatibility converter by default.
The Rust player is an opt-in Experimental setting. MP4 movies use native playback when supported;
MKV and unusual codecs use conversion by default. An iPhone cannot reach this project's
default `127.0.0.1` server on another computer. The proxy is intentionally localhost-only; **do
not expose it to a network by just changing its bind address**. A phone-accessible deployment
needs HTTPS and public-hosting security work first.

| Crate | What it is |
|---|---|
| `xtream` | API client and M3U channel-list reader: login, live/movie/series lists, episodes, stream URLs. Native and wasm. |
| `proxy` | Localhost pass-through proxy (browsers can't reach IPTV servers directly) that also serves the web app. |
| `app` | Dioxus web UI, compiled to WebAssembly. |

Video and sound handling is not in this repository: it is [rstreamkit](https://github.com/MoYusuf1/rstreamkit), a separate pure-Rust
library (HLS, MPEG-TS and MP4/MKV movie files in, fMP4 out, AC-3/E-AC-3/MP2 sound decoding in Rust, and the
MediaSource glue that plays it in a `<video>`). `app` pulls it from its private GitHub repo (cargo uses your git SSH
access, see `.cargo/config.toml`). To work on both at once, clone it next to this folder and build with
`--config 'patch."ssh://git@github.com/MoYusuf1/rstreamkit.git".rstreamkit.path="../rstreamkit"'`.

Everything you write is Rust. The only non-Rust file in the build output is the small JS loader that
`wasm-bindgen` generates (browsers can't start WebAssembly without one); it is never edited by hand.

## How it fits together

```text
 browser (WASM)                       your machine                      internet
 ┌───────────────────────┐  /proxy   ┌───────────────────┐            ┌────────────────┐
 │ app    screens, state │ ────────▶ │ riptv             │ ─────────▶ │ IPTV provider  │
 │ ├ xtream  API client  │  ?url=…   │ serves the app,   │  public    │ and its CDNs   │
 │ └ rstreamkit live TV  │ ◀──────── │ relays the bytes  │  addresses └────────────────┘
 └───────────────────────┘           └───────────────────┘  public hosts only
```

| Component | Job | Checked by |
|---|---|---|
| `xtream` | Speaks the Xtream Codes API and builds stream URLs, routing everything through the proxy when asked. Lists are decoded record by record as the response arrives, so a 30,000-title list peaks at ~8 MB of heap (the list itself), not ~25 MB with the whole body held first, and never as a ~70 MB JSON tree. | Unit tests on real-world JSON quirks |
| `rstreamkit` | Live: playlist → MPEG-TS demux → fMP4 → the browser's MediaSource. Movies and episodes: MP4 or MKV index → byte ranges → fMP4, sound decoded on the way. Only `mse` touches the browser; the rest is plain Rust that runs and tests natively. | Real HLS segments, MP4 and MKV files, decoded by `ffmpeg` |
| `proxy` (`riptv`) | The one native program. Serves the built app and relays requests for the app alone, to public addresses only, with `Range`, redirect checks and a strict CSP. Compression is left to the browser: it forwards what the page can decompress, so a provider that compresses its lists sends less over the slow hop. | End-to-end tests through a real proxy |
| `app` | Screens and state only, with no protocol or media code. | Driven in a browser |

Where a change belongs: protocol quirks in `xtream`, anything about video bytes in `rstreamkit`, anything
at the network boundary in `proxy`, and `app` only arranges them.

## Lean by using the browser

The page does little that the browser already does. Posters and logos load lazily and decode off the main
thread, and a TMDB picture is asked for at the size it is shown (a 140 px card no longer downloads and decodes
a 600x900 poster). Off-screen cards and channel rows are skipped by the browser's own layout
(`content-visibility`), which keeps a 4,000-card scroll about 2.5x cheaper to lay out. Fullscreen,
picture-in-picture, HLS where the browser has it, and decompression of provider lists are the browser's.
Nothing polls: the controls hide with one timer that only exists after activity, and the stream readout
runs only while it is open. Searching 30,000 titles allocates nothing per title.

`cargo test -p xtream --release -- --ignored --nocapture parse_cost` prints what reading a big list costs.

## Local operation

There is no host list to keep. The proxy relays only for the app itself (not for other
websites), and only to public addresses, so a provider, or a redirect it sends, can't point it at your
router or other machines. A server on your own network (or this machine) works because signing in
approves that one address. `cargo riptv` is an alias for `cargo run --release -p riptv --` (see
`.cargo/config.toml`); `IPTV_PORT` changes the port.

Open it in a normal browser (Chrome, Firefox, Safari). Sound and real fullscreen depend on the browser,
and an embedded preview pane may have neither; the player falls back to filling the page. On the live
player Space plays or pauses, F is fullscreen, M mutes, and double-click is fullscreen.

### Share a stream for local troubleshooting

Run `cargo riptv --logs` to append sanitized probe results to
`/tmp/riptv-diagnostics.log` (on systems with a different temp directory, the startup message
prints its path). Open a live channel, press its info button, then **Share for local testing**.
This creates an opt-in session lasting ten minutes. Either local chatbot can inspect it with:

```sh
curl http://127.0.0.1:3000/diagnostics
curl -X POST http://127.0.0.1:3000/diagnostics/SESSION_ID/probe
```

The list and report contain no stream URL, username, password, token, or provider response body.
The proxy keeps the URL only in memory, fetches at most 64 KiB from the stream (and up to two HLS
playlist links), and reports HTTP status, container signature, transport-stream sync and common
codec IDs. It never passes a credential-bearing URL to a subprocess. Stop the server to erase all
sessions. This is **local-only**: someone with access to your computer's loopback port can run a
shared probe while it is active. No stream is shared until you press the button.

## Try it without a provider

```sh
cargo run -p riptv --example mock_provider      # fake provider on :8081, login demo / demo
cargo riptv
```

Open the page and click "Try the demo". It has movies with AC-3 sound as an MP4 and as an MKV (served
with byte ranges, like a real provider, and played by rstreamkit), one with DTS sound that needs ffmpeg, a
two-season series whose episodes are the same two files, an HLS channel, a simulated live channel with
a sliding playlist window, and an anamorphic PAL channel (720x576 with 64:45 pixels) that must come out
16:9. Every channel has a generated day-long guide. `MOCK_MOVIE=/path/film.mkv cargo run -p riptv
--example mock_provider` adds your own file as a sixth movie, which is the best way to try seeking. `MOCK_TITLES=30000` adds that many
channels, movies and series (and `MOCK_PORT` moves it off 8081): a big provider's catalogue, for trying how the app copes.

## Profiles

RIPTV opens on **Who's watching?**: one tile for each account you have saved, a **+** to add another, and
**Edit** (**Manage profiles** on a desktop) to change or delete them. Tapping a tile signs in with that
account straight away. A profile is an Xtream account (server, username, password) or an M3U/M3U8
playlist, with a name and an automatically assigned avatar and colour, and you can keep as many of each as you like. **Switch profile** in
the account menu brings the tiles back.

Profiles are saved on the device, in this browser's storage for this address, **as plain text, the
password included**: that is what makes a tile sign in with one click. They never leave the machine
except to go to their own provider through the local proxy. Anyone who can read this browser's profile
can read them, so use **Delete profile** when an account shouldn't stay. They belong to the address you
open the app at (`127.0.0.1:3000` and `localhost:3000` keep separate lists).

## Add an M3U playlist

Add a profile and choose **M3U / M3U8**, paste an HTTP(S) playlist URL, and select **Save and connect**.
An IPTV `.m3u` channel list becomes Live TV categories and channels. A direct HLS `.m3u8` manifest
becomes one live channel; its video segments are not mistaken for separate channels. **Public TV**
on the profile chooser opens the [iptv-org all-channel playlist](https://iptv-org.github.io/iptv/index.m3u)
with one click, without entering credentials or saving a profile. Public streams can go offline or be
region-blocked; the all-channel list may take longer to load than a smaller playlist.

M3U mode currently covers Live TV only. Movies, series, and XMLTV guide data need separate metadata
support, so those tabs are hidden rather than showing empty pages. Xtream sign-in and its guide are
unchanged.

## Using it

The three sections (Live TV, Movies, Series) live on a floating rail, which becomes a tab bar at the
bottom on a phone; there the category list turns into a **Categories** button.

Live TV shows every channel as a tile grid until you pick a category; then it becomes a channel list
next to the player, with the channel's schedule as a timeline underneath on wider screens. On a phone,
the player stays above the channel list and the EPG is hidden to leave room for the picture. Every list can be sorted
(newest or oldest added, A to Z, Z to A, top rated). Movies and Series are poster
grids. Each section is loaded once, so category counts are exact and switching category is instant; the
account menu's **Refresh** reloads it. Live channels are endless streams, so they can't be downloaded.
On live TV, ↑/↓ or the player buttons change channel in the current category and sort order.
Type a channel number and wait a moment (or press Enter); on a phone, use the **123** button.

### Movie and series pages

Opening a title gives a full-page view under the floating chrome: a big backdrop (or a lightweight
gradient when artwork is missing), the title and
genres, **Play** (or **Resume**), the trailer and download buttons, the year, length, age rating and
quality, the rating, the director and the plot, and a card of facts (including when a movie would
end if you started now). Below are the cast, the trailer and, for a series, a season picker and a
swipeable row of episodes with thumbnails, lengths and summaries. Tapping an episode opens the full-page
player. Everything shown is what the provider's panel
knows; what it doesn't send is left out. Cast is shown as names only when the provider has no actor photos.

### The player

Movies and episodes play full page: back and title on top, a seek bar that shows what is buffered,
play/pause, 15-second skips, volume, time, playback speed, picture-in-picture and fullscreen. The
controls fade while you watch. Keys: Space or K plays and pauses, J/← and L/→ skip 15 seconds, ↑/↓
change the volume, M mutes, F is fullscreen, N is the next episode, Esc goes back. Where you stopped
is remembered in the browser (not sent anywhere), per profile, and offered the next time, with **Start over**;
when an episode ends the next one is offered, and starts in a few seconds unless you cancel.
The browser's Media Session API also connects supported headset, keyboard and lock-screen controls
to play/pause and seek; the next-episode key works when an episode follows. Volume, speed, last
category and sort are remembered per profile on this device.

Titles you have started and not finished appear in a **Continue watching** row at the top of Movies and
Series (a series shows the episode you were in, and its page's main button resumes that episode). The row
keeps the last 24 and only shows on the section's first page, with no category or search chosen. Only
ids and titles are kept, never stream addresses, which carry the account's credentials.

The heart on a movie or series page adds it to **My List** (up to 200 titles), a second row in the same
place. Both rows are kept per profile on the device.

## Compatibility mode (ffmpeg)

Many browsers, Chrome on Linux among them, can't decode some HEVC video, AC-3, MP2 or
AAC-Main sound, interlaced video smoothly, or raw MPEG-TS streams. When a stream is one of those the
player hands it to the proxy, which uses `ffmpeg` (if installed) to turn it into H.264 + AAC on the
fly: the video is copied untouched when it's already fine, while unsupported video is
re-encoded (NVENC if you have an NVIDIA card, otherwise x264), deinterlaced to full motion rate and
capped at 1080p (`RIPTV_MAX_HEIGHT=2160` for full 4K, if your browser decodes it smoothly). Without
ffmpeg those channels say why, and a sound-only problem still plays the picture. Everything else stays
in the pure-Rust player. ffmpeg follows redirects itself, so its own connections aren't held to the
public-address rule the proxy applies to the page's requests.

If audio arrives but no video frame does, RIPTV gives the picture another chance by forcing video
re-encoding even when the source claims to be browser-compatible. A source with no video track is
reported as audio-only; no player can reconstruct a picture the provider did not send. Audio stays
muted during video startup, then follows the saved volume setting when a frame appears or an
audio-only source is confirmed.

A converted movie or episode is one continuous stream with no index, which the browser can't seek in,
so its seek bar restarts the conversion from the chosen second (a moment's wait). Movies and episodes
are converted only when nothing in the page can play them (HEVC, DTS and TrueHD sound, interlaced
video), and that is decided before playback starts, never halfway through.

**Experimental player** is a switch in the account menu. On, live channels use rstreamkit in the
browser (including its AC-3/E-AC-3/MP2 decoding), and movies and episodes may use its Rust path.
It applies to the next stream you open and is remembered. Off is the default: native video or
ffmpeg compatibility mode handles playback. The decoder code remains in the web build.

Press `I` on the live player (or the info button) for the picture size, real frame rate, dropped
frames and buffer, which tells a slow stream from a slow decoder. When supported, the frame rate
counts frames actually presented by `requestVideoFrameCallback`; the counter only runs while the
overlay is open.

## What plays

**Movies and episodes** are probed before playback. The browser plays native-compatible files;
the proxy converts unsupported formats by default. Experimental mode can instead use rstreamkit:

| If the file is... | it is played by... |
|---|---|
| something the browser plays itself (H.264 with AAC, say) | the browser, as a plain `<video>` |
| H.264 with AC-3, E-AC-3, MP2 (or AAC where the browser can't read the container) | ffmpeg by default; rstreamkit only in Experimental mode |
| HEVC, DTS, TrueHD, interlaced, or anything it can't read | the proxy's ffmpeg (above), chosen up front |

Decoded sound is stereo (5.1 is mixed down) and is held uncompressed, which is a lot of memory for a
browser that keeps only about a minute of sound in a buffer, so the page reads a few seconds at a time. A server that doesn't answer byte-range requests, or a file it can't read the
index of, is left to the browser.

**Live TV** first tries native HTTPS HLS on browsers that support it, then ffmpeg. Experimental mode uses `rstreamkit`:
HLS with MPEG-TS segments carrying **H.264 video and AAC audio**. Anything else is reported, not
misplayed: HEVC, AES-128, fMP4 segments, continuous (non-HLS) `.ts` streams and AC-3/MP2 audio are not
played directly, and compatibility mode covers them (AC-3 and MP2 can instead be decoded in Rust,
see above). There is no adaptive bitrate: one variant is picked up front.

Release JS, Wasm and CSS assets with content hashes are cached as immutable for a year. The HTML
page, service worker, provider requests and video are never given that cache policy.

## Test

```sh
cargo test        # xtream and proxy (the wasm-only app is built with dx; rstreamkit has its own tests)
```

rstreamkit's own tests (`cargo test` in its folder) run real HLS segments through the transmuxer and have
`ffmpeg` decode the result (skipped if `ffmpeg` isn't installed).
