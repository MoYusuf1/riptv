# UI and performance review — 2026-10-29

This is a structural review, not a benchmark. Scores use the following scale: 0 broken or
unbounded; 1 inconsistent or fragile; 2 adequate with an evidenced issue; 3 coherent, bounded
and verified; 4 measured strong with repeatable evidence. These are engineering review scores,
not benchmark results or a claim of accessibility compliance. The baseline includes failures
found during browser testing. The after column describes the changes through `fcb3623`.
A domain can contain bounded components without total application memory being capped.

| Domain | Before | After | Evidence and principal cost |
| --- | ---: | ---: | --- |
| Catalog, search, sort, pagination | 1 | 3 | Cached sorting and page-bounded rendering were sound, but repeated category IDs could freeze section changes. Normalization and repeated large-catalog navigation now pass. Filtering still scans titles when a query changes. |
| UI, rendering, images | 2 | 3 | Page-bounded DOM and lazy images remain; added styles introduce no polling. Layout and focus flows passed browser checks. Image/GPU memory and interaction latency remain unprofiled. |
| Proxy, network, cache | 2 | 3 | Streaming and bounded queues remain; encrypted segment and key collection now have explicit byte limits and regression coverage. This is not a process memory cap. |
| Live and VOD lifecycle, buffering | 2 | 2 | Short browser playback and interaction flows pass. Real-device peak memory, repeated-session cleanup and long-watch behavior were not measured in this review. |
| Profiles, preferences, progress | 2 | 2 | Shelves and several preferences have limits; per-title progress and the profile list still have no overall history cap. |
| Design, accessibility, responsiveness | 1 | 3 | Neutral surfaces and a consistent font replace the old palette. Mobile navigation, picker layering, control clipping and focus issues were repaired and checked in-browser. Hardware checks remain. |

## Complexity and evidence

Let *n* be titles in a section, *L* the total characters inspected for a query, *p* the page
size, and *c* the number of categories. The whole catalog is loaded per visited section and kept
in memory until its cache is discarded (`app/src/main.rs`, `Library`, `Loaded`, `load`). This
cost is O(n) records plus their strings. Category counts take O(n) time and O(c) space.

- Search/category matching is O(n + L) per changed filter and O(n) indexes in the worst case;
  `matches` is memoized and a page redraw builds O(p) rows. Pages contain at most 60 grid or 40
  list rows (`app/src/main.rs`, `Browse`, `rows`, `PAGE_GRID`, `PAGE_LIST`). Sorting first builds
  lowercase keys and takes O(n log n) comparisons; ascending and descending indexes each use
  O(n) space and are cached (`app/src/catalog.rs`). Actual string comparison costs vary with
  title lengths. Category rendering scans categories and uses a pin set; it is not paginated.
- Visible catalog DOM is O(p); poster and thumbnail images request lazy, asynchronous decoding
  (`app/src/main.rs`, `Card` and row rendering). Browser image caches, GPU memory, and background
  components are outside that row count. No browser timing or heap measurement supports a
  claimed speedup.
- The ordinary proxy forwards upstream bytes as a stream (`proxy/src/lib.rs`, proxy response).
  The live path limits its queue to 32 owned chunks of at most 64 KiB and the startup sniff buffer
  to 4 MiB (`proxy/src/live.rs`). Playlist reads have a 1 MiB limit. These are component bounds,
  not a total process memory limit; current network chunks, FFmpeg, the browser, and concurrent
  streams add memory. Existing bounded caches and their tradeoffs are detailed in
  [performance.md](performance.md).
- Live and watch components have cleanup paths (`app/src/live.rs` and `app/src/watch.rs`,
  `use_drop`); the standard live fetch owns cancellation (`app/src/standard.rs`). Lifecycle
  correctness and memory over repeated real-device playback remain to be profiled.
- Shelves retain at most 24 recent titles and 200 saved titles; each update scans and serializes
  the shelf, O(s) for its size (`app/src/shelves.rs`). Preferences limit pinned categories to
  100 and live backups to 64 (`app/src/preferences.rs`). Progress uses a separate storage key
  per title and can grow with watch history; profile storage also has no explicit overall cap.
  Storage operations are synchronous browser operations, and quota/write failures are not measured.

## Changes and verification

The UI changes introduce neutral charcoal surfaces, a clearer type hierarchy, recoverable empty
states, top section navigation on phones, and a native category dialog. Primary chrome does not
use backdrop blur. The local Latin variable font adds **176,996 bytes** plus a 4,554-byte OFL
license to the built web assets; it makes no third-party font requests. Other writing systems use
system fallback fonts. The CSS and interaction changes add no polling.

The design rubric is separate from runtime benchmarking:

| Dimension | Acceptance gate |
| --- | --- |
| Palette and type | Consistent surface/text/accent roles; readable title, body and metadata hierarchy. |
| Layout | No viewport overflow; controls remain inside the viewport and the category picker stays above navigation. |
| Feedback | Loading, empty results, selection and error states have clear outcomes and recovery actions. |
| Keyboard | Open, activate and dismiss controls; restore focus; keep player shortcuts out of native controls and modals. |
| Responsive flows | Complete browsing, detail and playback flows at phone, tablet and desktop widths. |
| Runtime cost | No new polling; bounded response collection; explicitly document added assets and remaining measurements. |

Declared solid-color text/surface contrast ratios, calculated using relative luminance: primary
text/page **17.67:1**, secondary text/panel **8.49:1**, metadata/row **4.93:1**, white/action fill
**5.96:1**. These cover those token pairs, not every composited image, state or accessibility rule.

A browser crash found during this review was reproducible without playback: switching among
sections on the 30,000-title mock eventually panicked in the renderer. The mock returns duplicate
category IDs; these became duplicate keyed category items. Category methods now collapse repeated
IDs once at loading, preserving the first name and provider order, in average O(c) time and O(c)
temporary space. The same eight Live/Movies/Shows transitions passed after normalization with no
new browser errors. A native regression verifies conflicting duplicate names and ordering.

Encrypted HLS segments are now collected with a 64 MiB payload limit and AES keys with a 16-byte
limit, including chunked responses (`proxy/src/live.rs`, `read_bounded`, `segment_key`). CBC
decryption consumes the collected vector in place rather than copying the whole response. An
oversized or failed read follows the existing reopen path before forwarding plaintext. A legitimate
segment above 64 MiB will be rejected; the threshold is a compatibility tradeoff, not a measured
maximum for providers. Vector spare capacity and reallocation, an incoming chunk, queued output,
and the other components above mean peak memory can exceed 64 MiB per stream.

The project `.agents/check` passed. `cargo test --locked -p riptv -p xtream` passed, with two
synthetic benchmarks ignored. Focused tests cover chunked overflow, an exact-limit read, an
oversized declared length, 15/16/17-byte keys, and normal encrypted segments in order. They do
not measure peak allocation or exercise an actual 64 MiB segment, or prove overflow recovery
and cancellation across a full playback session.

## Browser acceptance evidence

Checks used the in-app browser, the local mock provider with `MOCK_TITLES=30000`, and a saved
real-provider profile for activation and browsing. Phone sizes below are CSS viewport widths,
not physical-device certification. Category normalization was verified without changing the
renderer's structure.

| Flow | Result |
| --- | --- |
| Saved profile activation | Library opens; contextual search is focusable; current palette and bundled font are applied. |
| Large-catalog navigation | Eight Live/Movies/Shows transitions pass with no new browser errors after category normalization. Next/Previous page changes the active page and remains responsive. |
| 320, 390, 820 and 1280 px browsing | No document horizontal overflow; 60-card pages; sidebar hidden at phone/tablet sizes and visible on desktop. |
| Search and categories | Long unbroken query wraps; clear search restores input focus; empty-result recovery returns the list. Category dialog occupies the native top layer. Close, Escape and selection restore focus, including grid/list changes. |
| Category keyboard actions | Typing and repeated pin/unpin inside the modal leave the live video paused and do not toggle fullscreen. |
| Live at 320 px | Picture spans exactly 0–320 px; controls have equal 320 px scroll/client widths. Settings fit in a bottom card; mode selection and explicit Close work and return focus. |
| Movie at 320 px | Mock video advances with readyState 4; all seven visible control rectangles fit. Settings Space does not toggle playback; Escape closes settings first. Native range ArrowRight changes 0 to 1 rather than triggering the global 15-second shortcut. |
| Movie focus lifecycle | Background header, workspace and detail are inert during playback, absent from the inert set afterward; player launch focuses Watch and dismissal restores Play/Resume. |
| Landscape settings, 667×320 | Speed menu remains inside the viewport; speed selection applies. |
| Show details | Season 2 displays the expected three episode cards; season selection returns focus to its opener. Episode-strip overflow stays inside the strip, without overflowing the page. |
| Account and sort | Section navigation dismisses the account menu. Escape closes either menu and returns focus to its trigger on the saved provider profile. |

Full-flow testing caught an additional framework-specific regression before push: a boolean
`inert: false` was serialized as a present `inert="false"` attribute, which still disables an
HTML subtree. Optional attributes now omit `inert` entirely outside playback, and launch/close
was retested. A compiler check alone did not establish the browser behavior.

These checks do not prove audio output on hardware, actual touch gestures, software-keyboard
resizing, notched-device safe areas, Picture in Picture support, or native fullscreen behavior
on iOS. Reduced-motion rules were preserved and reviewed; the preference was not changed on
a physical device.

## Next priorities

1. Test on real iOS and Android devices: software keyboard, touch scrolling and seeking, safe
   areas, fullscreen, Picture in Picture, and reduced motion.
2. Record browser interaction latency and peak memory for representative large catalogs and
   repeated live/VOD playback, with device, dataset and baseline; inspect image-cache behavior.
3. Exercise encrypted overflow, cancellation and reopening end to end, and validate the segment
   ceiling against representative providers before treating it as a compatibility guarantee.
4. Measure accumulated progress storage and define retention or cleanup if real usage warrants it.
