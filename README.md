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

## Run

```sh
dx build --web --release -p app                      # once, and after UI changes
IPTV_ALLOW=<provider host> cargo run --release -p riptv   # http://127.0.0.1:3000
```

`IPTV_ALLOW` is a comma-separated list of hostnames (no port) the proxy may reach, including any
hosts your provider redirects streams to (CDNs). Anything else is refused, and the app tells you
which host to add. The proxy listens on localhost only.

## Try it without a provider

```sh
cargo run -p riptv --example mock_provider      # fake provider on :8081, login demo / demo
IPTV_ALLOW=127.0.0.1,interactive-examples.mdn.mozilla.net,test-streams.mux.dev cargo run --release -p riptv
```

Open the page and click "Demo". It has a movie, a two-season series, an HLS channel,
and a simulated live channel with a sliding playlist window.

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
