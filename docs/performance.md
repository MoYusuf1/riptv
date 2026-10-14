# Runtime optimizations

Implemented in the source; no dependency upgrades or playback-quality reductions.

## First ten

1. Catalog search: compute matching indexes only when the catalog, query, category, or sort changes. Page redraws reuse them.
2. Sorting: cache ascending and descending indexes per immutable catalog. Only the first ascending sort normalizes names.
3. Series details: share the enriched episode snapshot and memoize playback entries. Comparing snapshots does not walk every episode.
4. Guide previews: inactive rows have no refresh timer; visible previews reuse a per-profile cache of at most 64 channels.
5. Cast photos: cap the public-data cache at 256 entries; fetch sequentially and cancel page-owned work on exit.
6. Pins: build a lookup set rather than repeatedly scanning the pin list for each category.
7. Startup: stream decompression directly to files instead of allocating a full unpacked resource.
8. Media checks: coalesce identical checks, cache results briefly, cap cache entries at 64, and allow at most four active checks. Authorization is still checked on each request. Failures are not cached.
9. Stream detection: inspect only new complete transport packets; retain at most 1 KiB of elementary-stream headers. Packet offsets remain relative to the original stream.
10. Live queues: bound each queued payload to 64 KiB, for at most 2 MiB across 32 slots. This excludes the producer's current network chunk, stream startup buffer, browser buffers, and FFmpeg's memory. Existing browser time/quota limits remain unchanged.

## Ten more

11. Current program: replace sorting the whole schedule on each lookup with two linear passes, preserving overlap and gap rules.
12. Guide zoom: select the middle program length without sorting all lengths.
13. Episode matching: index external episode information by season and episode instead of searching the full guide for each episode.
14. Provider JSON: move guide/series JSON subtrees into parsing instead of cloning them.
15. Episode queue: share one playback list and advance a cursor; do not clone or shift the remaining episodes.
16. Stream initialization: release the WASM initialization buffer once the browser has its copy.
17. JSON records: enforce the record-size limit even when an oversized record arrives complete in one chunk.
18. Diagnostics: compare stream URL components by borrowing rather than allocating host/path strings.
19. Preferences: skip serialization and storage writes when a setting did not change.
20. Shelves: reuse parsed shelf data while still reading the current storage string, so changes in other tabs and profiles remain visible.

## Costs and tradeoffs

- Filtering still scans titles once per changed query; cached indexes add memory proportional to catalog size. Refresh/profile exit discards catalog caches.
- Pin checks become average O(categories + pins), episode matching O(provider episodes + guide episodes), and current-program lookup O(programs), instead of repeated scans or sorting.
- Sequential photo fetching lowers background work but can show later portraits more slowly.
- A fresh media check can wait behind the four-check limit. Successful live/radio checks expire after two seconds; checks with a known duration after thirty seconds. Entries are in-memory only.
- Cache and byte limits do not imply a total application memory cap.

## Verification

Regression tests cover paging and cached sorts, guide overlap equivalence, arbitrary stream chunk boundaries, oversized JSON records, concurrent media checks, bounded queued payloads, profile isolation, cross-tab shelf changes, and unchanged preferences. Existing conversion and seeking tests remain enabled.

Latest synthetic native release benchmark: 1,000 lookups against 1,000 guide entries took **88.39 ms** with the previous sorted-timeline lookup and **34.59 ms** with direct lookup (about **2.6× faster**). Results were identical. This is not an overall browser/app speedup or a measured RAM reduction; individual timings vary between runs.

Reproduce:

```sh
cargo test --locked -p riptv -p xtream
cargo clippy --locked -p riptv -p xtream --all-targets -- -D warnings
cargo test --locked -p xtream --release guide_cost -- --ignored --nocapture --test-threads=1
dx build --web --release --locked -p app
```

Browser interaction and real-provider peak RAM still need profiling; no claim is made that those were measured here.
