# UI and performance review — 2026-10-29

This is a structural review, not a benchmark. Scores use the following scale: 0 broken or
unbounded; 1 inconsistent or fragile; 2 adequate with an evidenced issue; 3 coherent, bounded
and verified; 4 measured strong with repeatable evidence. These are engineering review scores,
not benchmark results or a claim of accessibility compliance. The baseline includes failures
found during browser testing. The score table records the first pass through `fcb3623`; subsequent refinement evidence appears below.
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
cost is O(n) records plus their strings. Category counts are no longer calculated or retained;
page totals come from the memoized filtered index length.

- Search/category matching is O(n + L) per changed filter and O(n) indexes in the worst case;
  `matches` is memoized and a page redraw builds O(p) rows. Pages contain at most 60 grid or 40
  list rows (`app/src/main.rs`, `Browse`, `rows`, `PAGE_GRID`, `PAGE_LIST`). Sorting first builds
  lowercase keys and takes O(n log n) comparisons; ascending and descending indexes each use
  O(n) space and are cached (`app/src/catalog.rs`). Actual string comparison costs vary with
  title lengths. Category rendering scans categories and uses a pin set; it is not paginated.
- Live channel actions reuse that same filtered index list. Up/Down scan O(m) matched indexes
  with O(1) extra memory; numbered selection is O(1), with a final checked catalog lookup.
  They no longer rescan and lowercase every title on each action. Removing category counts also
  removes one O(n) load-time pass and an O(c) retained map per loaded section.
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

## Mobile and playback refinement acceptance

The mobile catalog uses a plain header, large section title, two poster columns and a floating
section switcher containing only Live, Movies and Shows. Live opens a category page first; selecting a category opens its channels with a Back button.
The control beside search opens a compact sort-only sheet. There is no descriptive subheading or
category count. The catalog scrolls as one page, including its 44px pagination controls.
Profile monograms stay circular inside the rounded account control, with a quiet tint.

| Concern | Implementation and evidence |
| --- | --- |
| Missing guide enlarges picture | The player no longer grows into the guide's space. At 1280×600 its rectangle is identical with a populated guide and absent guide: approximately 626×318px. A short viewport can shrink the picture region; video remains contained. |
| Live/Smooth menu | Two explicit option rows with short descriptions and selected check; each measures over 60px high on a 320px phone viewport. Toolbar button styling is retained. |
| Stream information | Semantic label/value rows, units and tabular figures replace the debug string. The first frame-rate sample remains unknown until a sampling interval exists; sampling stops when closed and unchanged values do not trigger writes. Mobile panels cover the bottom navigation while open. |
| Movie startup | Continuous CSS rotation and a short status replace guessed percentages. A mock provider with 2s response latency shows Preparing video with no numeric progress value. Startup feedback clears on actual playback; the artificial completion delay is removed. Launch/close/relaunch restores focus and clears background inert state. |
| Category pins | Trailing targets measure 44×44px, rows 56px. Repeated pin/unpin preserves the active category and button focus. Header category search and All stay fixed while rows scroll. |
| Profile dismissal | Capture-phase document listener closes on navigation and search clicks, including controls that stop bubbling. Listener registration and cleanup match. Escape still restores focus. |
| Mobile layout | Effective 320px CSS viewport has two approximately 139px poster columns, no horizontal overflow, and section targets over 44px high. At 410px, posters remain in two columns. The title sits above search; its adjacent sort icon opens the sheet; dismissal restores trigger focus. |
| Landscape playback | At approximately 666×320px, Live occupies the full viewport and header, channels and section navigation are hidden. At 320px portrait, the player is 320×180px and controls have equal 320px client and scroll widths. |
| Pagination | Previous/Next use 44px targets with a clear page label; activation changes both the page and title list. Pagination belongs to the scrolling catalog and is clear of the fixed section switcher. |

Independent code review found and resolved capture-listener propagation, sidebar selector and
category flex sizing issues. Browser review caught a floating filter control covering pagination;
the final filter control is beside search. Compilation alone is not acceptance evidence.
The implementation and review are delegated separately; the integrating agent owns cross-component
browser validation. The same 0–4 rubric above applies to each follow-up batch.

Mobile Live navigation was additionally checked at a 320px CSS viewport: category search →
category → channel → moving mock video, then Back removes the video and returns focus to category
search. All → filtered channel → stream also passes. Returning to another category clears the
old channel query. Empty category-search recovery clears the query and focuses the visible
header search, rather than a hidden duplicate filter. The header search and sidebar filter have distinct IDs; later refinement below removes the duplicated category tree.
The sort sheet mounts no category list; its A–Z/Z–A controls use the same capsule treatment as
the section switcher. Desktop browsing retains its sidebar and channel workflow.

Navigation parity tests cover empty results, missing current channels, wraparound, numbered bounds,
duplicate identifiers and stale indexes. The ignored native benchmark can be rerun with:

```sh
cargo test --locked -p riptv --test catalog --release navigation_cost_30k -- --ignored --nocapture --test-threads=1
```

One run over 30,000 titles, 500 filtered hits and 600 actions measured 32.401ms for refiltering each
action versus 302.821µs for reusing filtered indexes, with identical outputs. This synthetic native
measurement excludes initial filtering and does not establish browser latency or total-memory gains.

The project check, native tests, native/WASM Clippy with warnings denied, and release web build pass.
A corrupted incremental compiler cache caused one compiler crash; the same WASM Clippy command
passes with `CARGO_INCREMENTAL=0`. Native fixture tests require loopback
access; the restricted-sandbox attempt fails to bind sockets and the loopback-enabled run passes.
Real phone touch, keyboard, safe-area and hardware audio checks remain outstanding.

## Follow-up reliability and category work — 2026-10-30

Category sorting now affects the visible category page as well as the channel order. The ordered
category snapshot is memoized by category data, section and sort; search and pin changes reuse it.
This adds O(c log c) sorting and O(c) category/key storage per changed snapshot, rather than sorting
on every keystroke. Pinned categories remain first, ordered within each group.

The sidebar and mobile root now share one `CategoryList` tree. Browser DOM checks show one tree
at both 320px and 1280px; CSS changes its placement, rather than constructing a hidden second list.
Empty-filter recovery focuses the visible search field at either breakpoint. Pin actions have
stable control IDs and restore focus after rows move.

Live fullscreen now belongs to a per-player controller. Toggle, Escape and player disposal cancel
stale fallback work; delayed tasks hold weak references. Native request Promises remain tracked
after the fallback deadline, so a cancelled late success exits only its original fullscreen player
and an old response cannot override a new request. Normal late native entry clears page-fill state.
Run `cargo test --locked -p riptv --test fullscreen --test catalog` for the targeted regressions.
Ten dependency-free transition tests cover cancellation, replacement, native changes, fallback,
late settlement and disposal. The browser is additionally checked for rapid toggles, Escape and
category Back cleanup; those checks do not certify iOS native fullscreen or simulate every possible
browser Promise delay. There is no new polling loop.

The MAST audit, implementation and independent review are separate tasks. Review caught the
late-native-response case after the initial fallback fix; browser interaction caught focus loss
when a pin moved its row. Checks and targeted regression tests accompany the integrated change.

## Next priorities

1. Test on real iOS and Android devices: software keyboard, touch scrolling and seeking, safe
   areas, fullscreen, Picture in Picture, and reduced motion.
2. Record browser interaction latency and peak memory for representative large catalogs and
   repeated live/VOD playback, with device, dataset and baseline; inspect image-cache behavior.
3. Exercise encrypted overflow, cancellation and reopening end to end, and validate the segment
   ceiling against representative providers before treating it as a compatibility guarantee.
4. Measure accumulated progress storage and define retention or cleanup if real usage warrants it.
