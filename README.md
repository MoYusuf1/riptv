# RIPTV

RIPTV is an Xtream Codes IPTV client, all Rust and small enough to ignore: the web app is about
0.7 MB before compression, and the proxy is a 2.9 MB binary that idles at ~4 MB of RAM on two threads.

| Crate | What it is |
|---|---|
| `xtream` | API client: login, live/movie/series lists, episodes, stream URLs. Native and wasm. |
| `player` | HLS + MPEG-TS to fMP4 transmuxer and the MediaSource glue that plays it in a `<video>`. |
| `proxy` | Localhost pass-through proxy (browsers can't reach IPTV servers directly) that also serves the web app. |
| `app` | Dioxus web UI, compiled to WebAssembly. |

The live player also loads the selected channel's short Xtream EPG on demand, showing its current
and upcoming programmes without downloading guide data for the rest of a large channel list.

Everything you write is Rust. The only non-Rust file in the build output is the small JS loader that
`wasm-bindgen` generates (browsers can't start WebAssembly without one); it is never edited by hand.

## How it fits together

```text
 browser (WASM)                       your machine                      internet
 ┌───────────────────────┐  /proxy   ┌───────────────────┐            ┌────────────────┐
 │ app    screens, state │ ────────▶ │ riptv             │ ─────────▶ │ IPTV provider  │
 │ ├ xtream  API client  │  ?url=…   │ serves the app,   │  only for  │ and its CDNs   │
 │ └ player  live TV     │ ◀──────── │ relays the bytes  │  hosts in  └────────────────┘
 └───────────────────────┘           └───────────────────┘  allowlist
```

| Component | Job | Checked by |
|---|---|---|
| `xtream` | Speaks the Xtream Codes API and builds stream URLs, routing everything through the proxy when asked. Lists are decoded record by record straight from the response bytes: a 30,000-title list peaks at ~7 MB of heap instead of ~70 MB. | Unit tests on real-world JSON quirks |
| `player` | Playlist → MPEG-TS demux → fMP4 → the browser's MediaSource. Only `mse` touches the browser; the rest is plain Rust that runs and tests natively. | Real HLS segments, decoded by `ffmpeg` |
| `proxy` (`riptv`) | The one native program. Serves the built app and relays requests to allowlisted hosts, with `Range`, redirect checks and a strict CSP. | End-to-end tests through a real proxy |
| `app` | Screens and state only, with no protocol or media code. | Driven in a browser |

Where a change belongs: protocol quirks in `xtream`, anything about video bytes in `player`, anything
at the network boundary in `proxy`, and `app` only arranges them.

## Run

```sh
rm -rf target/dx && dx build --web --release -p app  # once, and after UI changes (dx never deletes old builds)
cargo riptv <provider host>                          # http://127.0.0.1:3000
```

The arguments are the hostnames (no port) the proxy may reach: your provider, plus any hosts it
redirects streams to (CDNs). `IPTV_ALLOW` (comma-separated) adds more. Anything else is refused, and
the app names the host to add. The proxy listens on localhost only. `cargo riptv` is an alias for
`cargo run --release -p riptv --` (see `.cargo/config.toml`).

## Try it without a provider

```sh
cargo run -p riptv --example mock_provider      # fake provider on :8081, login demo / demo
cargo riptv 127.0.0.1 interactive-examples.mdn.mozilla.net test-streams.mux.dev
```

Open the page and click "Try the demo". It has a movie, a two-season series, an HLS channel, a
simulated live channel with a sliding playlist window, and an anamorphic PAL channel (720x576 with
64:45 pixels) that must come out 16:9. Every channel has a generated day-long guide.

## Using it

The three sections (Live TV, Movies, Series) live on a floating rail, which becomes a tab bar at the
bottom on a phone; there the category list turns into a **Categories** button.

Live TV shows every channel as a tile grid until you pick a category; then it becomes a channel list
next to the player, with the channel's schedule as a timeline underneath. Movies and Series are poster
grids, newest first; opening one shows its page (backdrop, plot, cast, and Play or the episodes).
Each section is loaded once, so category counts are exact and switching category is instant; the
account menu's **Refresh** reloads it. Movies and series episodes have a download button (a plain
browser download through the proxy). Live channels are endless streams, so they can't be downloaded.

## What plays

Movies and series are plain files played by the browser. Live TV goes through `player`:
HLS with MPEG-TS segments carrying **H.264 video and AAC audio**. Anything else is reported, not
misplayed: HEVC, AES-128, fMP4 segments and continuous (non-HLS) `.ts` streams are not supported, and
AC-3/MP2 audio plays as video only with a note. There is no adaptive bitrate: one variant is picked up front.

## Test

```sh
cargo test        # xtream, proxy, player (the wasm-only app is built with dx)
```

The player tests run a real HLS segment through the transmuxer and have `ffmpeg` decode the result
(skipped if `ffmpeg` isn't installed).
