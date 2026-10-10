# RIPTV: technical breakdown and lessons learned

This is a handoff document for an AI assistant (or a developer) starting work on RIPTV. Read it
completely before you change anything. It covers:

- what the app is;
- the tools and stack it uses, and how the pieces fit together;
- how to build, run, test and release it;
- the rules the owner holds the code to;
- the hard-won lessons: bugs that took real debugging to find, and their causes.

Repository: <https://github.com/MoYusuf1/riptv>. The code is about 13,000 lines of Rust.

---

## 1. What RIPTV is

RIPTV is a lightweight IPTV client: live TV, movies and series. It accepts three kinds of source:

- an **Xtream Codes** account (server, username and password);
- an **M3U/M3U8 playlist**;
- **Public TV**, the iptv-org public playlist, opened with one click.

It ships as one native executable (`riptv`), which does three things:

- it serves the web UI (Rust compiled to WebAssembly) at `http://127.0.0.1:3000` and opens the browser;
- it relays requests to the provider, because browsers can't reach IPTV servers directly (CORS,
  mixed content and odd headers);
- it runs **ffmpeg/ffprobe** as subprocesses to convert streams that the browser can't play natively.

Release downloads bundle a minimal ffmpeg 8.1.3 built by `scripts/build_ffmpeg.sh` (published as a pre-release; pinned in `scripts/ffmpeg.json`), so users need nothing else installed.

---

## 2. Stack and tools

### Languages and core crates

| Piece | Tool | Notes |
|---|---|---|
| Language | Rust, edition 2024, stable toolchain | Everything is Rust. The only JS is the loader that wasm-bindgen generates. |
| UI | **Dioxus 0.7.10** (web target) → `wasm32-unknown-unknown` | Built with the Dioxus CLI `dx` 0.7.10, never with plain `cargo build`. |
| Browser APIs | `web-sys`, `wasm-bindgen-futures` | Covers MediaSource, SourceBuffer, video and media elements, Storage, AbortController and fetch streaming. |
| Server | **Axum 0.8** + **Tokio** | Minimal features (`http1`, `query`, `json`). Uses `tower-http` for static files and headers. |
| HTTP client | **reqwest 0.13** with rustls | Streams responses. No OpenSSL in the server. |
| Data | `serde` / `serde_json` | The `raw_value` feature lets `xtream` decode records one at a time instead of building a whole tree. |
| Media conversion | **ffmpeg / ffprobe** subprocesses | Pinned in `scripts/ffmpeg.json` with SHA-256 hashes. |
| Experimental media engine | **rstreamkit**, a separate repo pulled over git | Pure-Rust HLS/TS/MP4/MKV → fMP4 and AC-3/E-AC-3/MP2 decoding. **Owned by another agent: never edit it.** |

### Workspace layout

```
Cargo.toml          workspace; default-members = xtream, proxy (app is wasm-only)
.cargo/config.toml  alias: `cargo riptv` = run --release -p riptv --
xtream/             Xtream API + M3U client (native AND wasm), stream sniffing (sniff.rs)
proxy/              binary `riptv`: Axum server, /proxy relay, /live, /compat, diagnostics, updates, /quit
  src/live.rs       live pipeline: follows HLS itself → ffmpeg (stdin) → fMP4
  src/compat.rs     movie/episode conversion (/compat), ffprobe, NVENC check, ffmpeg args
  src/diagnostics.rs  --logs tracing and the probe endpoint
  tests/proxy.rs    end-to-end tests through a real proxy, with real ffmpeg
  examples/mock_provider  fake Xtream provider for offline testing
app/                Dioxus UI (wasm)
  src/main.rs       screens and state (large; keep new logic OUT of it, use modules)
  src/standard.rs   stable live player: MSE pump fed by /live
  src/playback.rs   engine choice for titles: Native vs Convert
  src/live.rs       live TV screen
  src/profiles.rs   "Who's watching?" profiles (localStorage)
  src/timeline.rs, shelves.rs, categories.rs, controls.rs, media_session.rs, quit.rs, updates.rs …
scripts/            setup.sh / setup.ps1 / setup.cmd, package.py, smoke_package.py, release_sources.py
.github/workflows/  release.yml ("Release downloads"), windows.yml ("Windows setup")
docs/               reference.md (architecture), live-playback.md (measurements), setup.md, release-notes.md
```

**Where each change belongs:**

- protocol quirks go in `xtream`;
- network-boundary and ffmpeg work goes in `proxy`;
- bytes-to-fMP4 work in Rust goes in `rstreamkit` (not ours);
- `app` only arranges screens and state.

### Proxy HTTP routes (`proxy/src/lib.rs`)

| Route | Job |
|---|---|
| `GET /proxy?url=…` | Relays a provider request. App only, public addresses only, `Range` passed through, every redirect checked. |
| `POST /allow` | Signing in approves one private or LAN server address. |
| `GET /live` | Live channel → fMP4 stream (the stable live player). |
| `GET /compat`, `/compat/check` | Movie/episode conversion (`start=` for resume/seek, `video=copy` or transcode). |
| `POST /quit` | Graceful shutdown. Refused unless the request comes from the app. |
| `/updates` | GitHub release check. |
| `/diagnostics…` | Only with `--logs`: lists sessions, probes one and records events. |

### Dev tools used while building

| Tool | What we used it for |
|---|---|
| `dx build --web --release -p app` | Builds the UI into `target/dx/app/release/web/public`, which the server serves. |
| `cargo test -p xtream -p riptv` | Native unit and end-to-end tests. Most tests run real ffmpeg. |
| `cargo clippy --target wasm32-unknown-unknown -p app` | **Run clippy for the wasm target too**: the app crate doesn't build natively. |
| `cargo riptv --logs` | Writes a trace to `/tmp/riptv-diagnostics.log` (Windows: `%TEMP%`). The most useful debugging tool in the project. |
| `ffprobe -show_frames / -show_packets` | Checks A/V sync on converted output: compare the first audio frame's `duration` and the end times of the audio and video streams. |
| A browser automation pane (Chrome) | Drives the real UI: drags on the seek bar, reads `video.currentTime` and `buffered`, takes screenshots. |
| `mock_provider` example | A fake Xtream server. `MOCK_PORT`, `MOCK_MOVIE=/path/file.mkv` (best for testing seeking) and `MOCK_TITLES=30000` (catalogue scale). |
| `gh` CLI | Issues, releases and workflow runs. |
| `fuser -k 3000/tcp`, `pkill -x riptv` | Kill a stray server holding the port. |

---

## 3. Build, run, test, release

```bash
./scripts/setup.sh              # first time: checks prereqs, wasm target, dx 0.7.10, builds all
cargo riptv                     # start (serves the already-built web app)
cargo riptv --logs              # start with tracing
dx build --web --release -p app # rebuild UI after app/ changes (then reload the page)
cargo test -p xtream -p riptv   # native tests (needs ffmpeg on PATH)
IPTV_PORT=3100 cargo riptv      # second instance on another port for testing
cargo run -p riptv --example mock_provider   # fake provider on :8081, login demo/demo
```

### Releasing

1. Bump `version` in `proxy/Cargo.toml` and refresh `Cargo.lock`.
2. Write the user-facing text in `docs/release-notes.md`.
3. Commit, tag `vX.Y.Z` and push the tag.

The tag starts two workflows:

- **Release downloads** builds Windows, Linux and macOS (Apple Silicon and Intel). It bundles
  verified ffmpeg, runs a smoke test on the extracted package, runs the tests against the bundled
  ffmpeg, and publishes only if everything passes.
- **Windows setup** checks the build-from-source path, in a directory whose name contains spaces.

**Commit convention:** plain messages with no AI co-author or attribution trailer (the owner's rule).

---

## 4. How playback works (the heart of the app)

There are **two engines, kept strictly separate**:

1. **Standard** is the default and must be "uber stable". It uses the browser's own decoders,
   with ffmpeg in the proxy whenever the browser can't handle the stream.
2. **Experimental** is rstreamkit: pure-Rust demux/remux in wasm into MediaSource. It is opt-in.

The two engines must never leak into each other: no shared ad-hoc branching, and a separate module
for each.

### Live TV (standard)

```
provider HLS ──▶ proxy/src/live.rs ──▶ ffmpeg (stdin → fMP4 stdout) ──▶ /live ──▶ app/src/standard.rs (MSE pump) ──▶ <video>
```

**In the proxy:**

- The proxy follows the HLS playlist **itself**, over one connection, instead of letting ffmpeg's
  HLS reader do it. ffmpeg's reader re-fetched the playlist and probed for 3 s, and a second
  connection makes single-connection providers drop the first one.
- Playback starts `START_SECONDS = 18` back from the live edge, for resilience.
- Codecs are sniffed in Rust (`xtream::sniff`).
- The stream starts at the first keyframe, with PAT/PMT prepended, because segments often begin
  mid-GOP and ffmpeg then fails with "dimensions not set".

**ffmpeg handling:**

- **Video** is copied when it is H.264 progressive. HEVC is transcoded (NVENC when it works).
  Interlaced 1080i is deinterlaced and transcoded.
- **Audio is always re-encoded to AAC.** Copying AAC randomly broke Chrome's audio decoder
  (`PIPELINE_ERROR_DECODE`), and the codec sniff can't predict when. Encoding costs about 1% of a core.

**Lifetime:** every task is aborted when the HTTP response is dropped. If leaving a channel doesn't
stop the playlist reader, the provider answers 509 (connection limit) on the next channel.

**Retries:** temporary failures are retried twice, serially, with backoff. A 509 closes immediately.
A partially delivered segment is never replayed into the decoder.

**The app side (`standard.rs`):**

- An explicit MediaSource buffer keeps up to 30 s ahead and trims played data.
- It waits for 6 s of buffer before starting, or before resuming after an underrun.

**Measured start times:** typically 1–3 s, down from 4–24 s or outright failure (see
`docs/live-playback.md`).

### Movies and episodes (standard)

`playback.rs` chooses the engine:

- `Native` when the browser can play the file (MP4 with H.264 and AAC);
- otherwise `Convert` through `/compat`, using ffprobe and then ffmpeg into fragmented MP4.

Resume and seek re-request `/compat?start=N`. **Read lesson 5.1 before you touch the seek arguments.**

---

## 5. Lessons learned (the expensive ones)

### 5.1 A/V desync after resume or seek (audio ~10 s ahead of the picture)

**Symptom:** converted movies played with the sound up to about 10 s ahead of the picture, but only
after a resume or a seek.

**Cause:** with `-ss` placed before `-i` and the video *copied*, ffmpeg can't cut video mid-GOP. The
video therefore starts at the previous keyframe, while accurate seeking cut the audio precisely.
On top of that, `aresample=async=1:first_pts=0` padded the gap into **one long first audio frame**,
and Chrome plays that frame straight through.

**Fix (`proxy/src/compat.rs`):**

- When the video is copied, add `-noaccurate_seek` before `-ss`, so audio and video both start at
  the keyframe.
- Use `aresample=async=1` with no `first_pts=0`.
- When transcoding, keep accurate seeking.

**Verification:** use ffprobe to check that the first audio frame is shorter than 0.1 s and that the
audio and video ends agree within 0.1 s. The regression test is
`a_resumed_conversion_keeps_sound_and_picture_together`. It uses a 60 s file with a 125-frame GOP and
AC-3 audio, and it fails on the old flags.

**Lesson:** verify sync with ffprobe numbers, not by eye, and write the test against a file whose
keyframe interval is long enough to expose the bug.

### 5.2 Seek-bar scrubbing froze or jumped (took three attempts)

The seek bar is an `<input type="range">`. Facts about its events that we learned the hard way:

- The event order is `pointerdown → input… → pointerup → change`, so **`pointerup` fires before
  `change`**. Seeking on pointerup therefore uses a stale value.
- `change` fires **only if the value moved**. A click that doesn't move the thumb produces no
  `change`, so the "scrubbing" flag never clears and the bar freezes.
- While you drag, the film keeps playing, so "did the value move?" must be measured against the
  value **when the drag started** (`scrub_from`), not against the live position.

**The fix (`app/src/main.rs`):**

- `input` sets `scrubbing=true` (recording `scrub_from` on the first event) and moves the thumb.
- `change` ends the scrub, seeks, and blurs the slider.
- `pointerup`, `pointercancel` and `blur` call `let_go()`. It only **unfreezes** the bar, if the value
  barely moved, and never seeks.

**Related controls bug:** the auto-hidden control bar uses `pointer-events:none`, so a press on
faded controls hit the video instead. Track `over_controls` with mouseenter and mouseleave, and keep
the controls shown while the player is active, scrubbing, or under the pointer.

**Lesson:** for any custom media control, write down the real browser event order first, and test
with real drags in a real browser: fast, slow, click without moving, release outside the bar, and
drag while the controls are fading.

### 5.3 Dioxus 0.7 signal borrow panics

- A `signal.peek()` or `signal.read()` guard **lives until the end of the statement**. So
  `x.set(f(*x.peek()))`, or any write to the same signal inside that statement, panics with
  `AlreadyBorrowed`.
- **Always read into locals first:** `let (at, from) = (*pos.peek(), *scrub_from.peek());`, then write.
- Raw JS event listeners (closures attached through web-sys) must **not** call Dioxus `spawn` or an
  `EventHandler`, because they run outside the runtime. Use `wasm_bindgen_futures::spawn_local`
  together with signals.
- Every `web-sys` API needs its feature enabled in `app/Cargo.toml` (for example, `class_list()`
  failed for lack of `DomTokenList`). Prefer what is already enabled, such as `class_name()`.

### 5.4 Providers are hostile environments

- Many providers allow **one connection per channel**. Any second request (a parallel codec check,
  a diagnostic probe) kills the viewer's stream. Sniff from the bytes you're already reading.
- Expect 509 (connection limit), 503 (overloaded), short text bodies instead of video
  ("Cannot read …"), playlists that take 8–13 s to answer, segments that start mid-GOP, and
  E-AC-3/AC-3 audio the browser can't decode.
- ffmpeg's `nobuffer` flag dropped every frame of interlaced streams. 4K streams needed a longer
  probe than ffmpeg's default half second before the first audio.
- Buffering can't fix sustained slow delivery. The logs separate `upstream_s` (the provider or the
  network) from `downstream_wait_s` (our pipeline) so you can blame the right side.

### 5.5 Shutdown and processes

- Release builds had no way to quit short of a system monitor. **Quit** in the account menu now
  sends `POST /quit`. That fires a `tokio::sync::Notify`, which triggers Axum's
  `with_graceful_shutdown`.
- Open streams never finish on their own, so shutdown is **capped at 2 s** after the signal.
- "Address already in use" almost always means a stray `riptv` is still running: `pkill -x riptv`.

### 5.6 Diagnostics first, guessing second

- `--logs` traces every open, engine choice, stall, media error, ffmpeg slowdown and provider refusal.
- Nothing in the log names a URL, username, password or host; text that might contain one is redacted.
- A post-failure probe gives a verdict such as `channel_offline_or_removed` or
  `provider_too_slow_for_live`.
- Most of the real bugs were found by reading this trace from real channels, not by reasoning.

### 5.7 Testing discipline

- **Test on real sources.** Use the owner's Xtream account or Public TV. The mock provider is for
  repeatable regression cases (a seekable movie via `MOCK_MOVIE`, catalogue scale), not for
  claiming that something works.
- When the provider is down, use Public TV for live channels and the mock provider with a real local
  video file for titles.
- When a fix involves ffmpeg, add an end-to-end test in `proxy/tests/proxy.rs` that runs real ffmpeg
  and asserts measured numbers.
- Building the UI with `dx build` changes what any running server on that output directory serves.
  Test on a second port when someone else's server is running.

---

## 6. Product and code rules (non-negotiable)

- **Fast and light.** Optimal time and memory complexity:
  - stream-decode large lists (a 30,000-title list peaks at about 8 MB);
  - don't poll;
  - let the browser do lazy images, `content-visibility`, fullscreen, PiP and decompression.
- **Modular, no spaghetti.** New logic goes in its own module, not in `app/src/main.rs`. The standard
  and experimental engines stay separate.
- **UI is minimalist, iOS-like:**
  - no eyebrows, sub-headers or explanatory text;
  - no extra buttons;
  - loading is a ring with no text, and status text goes only to the `--logs` trace;
  - transitions are smooth.
- **Security:**
  - the proxy is localhost-only and relays only for the app, only to public addresses (plus the
    one server a sign-in approves);
  - don't widen the bind address;
  - profiles, passwords included, are deliberately stored as plain text in browser storage, which
    is the owner's choice.
- **Don't touch `rstreamkit` or `rffmpeg`.** Another agent owns them. To work against a local copy,
  use a cargo `[patch]` passed through `--config`.
- Commits are plain, with no AI attribution trailers. Releases follow the steps in section 3.

---

## 7. Starting work: a checklist for the next AI

1. Read `docs/reference.md` (architecture) and `docs/live-playback.md` (measurements). Then skim
   `proxy/src/lib.rs`, `proxy/src/live.rs`, `proxy/src/compat.rs`, `app/src/standard.rs` and
   `app/src/playback.rs`.
2. Run `./scripts/setup.sh`, then `cargo riptv --logs`, and open a Public TV channel. Watch
   `/tmp/riptv-diagnostics.log`.
3. Before a change: decide which crate it belongs in (section 2).
4. After a change:
   - run `cargo test -p xtream -p riptv`;
   - run clippy for the wasm target;
   - run `dx build --web --release -p app`;
   - verify in a real browser on a real source.
5. For player UI changes, re-test the scrubbing cases in 5.2. For conversion changes, re-check sync
   with ffprobe as in 5.1.
