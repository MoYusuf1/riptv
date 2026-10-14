# Live recovery

Implemented locally for the standard live player. Movie and episode seeking use their existing, separate player. Experimental and native-HLS playback do not use this recovery ladder.

## When a stream fails

1. Keep playing usable buffered video. Tell missing input apart from a frozen picture, intentional pauses, and hidden-tab suspension.
2. Try one small seek inside buffered video for a stuck decoder. Cross at most two buffered gaps, each no longer than 15 seconds. No extra connection or fragment cache.
3. Reopen with increasing delays and jitter. Abort the old player fetch first. Stop after five automatic attempts; user pauses cancel pending work.
4. On the second attempt, try TS format once (Xtream accounts only). Never guess another address for a custom playlist.
5. On the fourth attempt, try one backup chosen by the user. Show its name and guide. Never automatically switch back or follow a chain of backup mappings.

Failure history clears after a healthy minute. Format changes, backup switching, local repairs, and gap jumps stay bounded for the channel session, even across manual retries.

## Provider safeguards

- Respect `Retry-After` seconds and HTTP dates, including failures after video starts. Provider-wide cooldowns also cover manual retries and format changes. HTTP 429/509 without a wait time gets a five-second cooldown; 401/403 require user action.
- Retry temporary requests only before forwarding their body. Never replay a partially delivered segment. Skip at most two missing 404/410 segments per connection.
- Refresh an expired playlist address once. Continue the decoder only when an identical overlapping segment confirms continuity; otherwise start fresh. Changed playlists wait a full target duration; unchanged ones half, following [HLS reload rules](https://www.rfc-editor.org/rfc/rfc8216.html#section-6.3.4).
- Playlist reads have byte and time limits. Cooldown storage holds at most 64 origins; overflow conservatively delays all providers rather than dropping an active limit. Wait values are capped at seven days.

## Controls and cost

Open the player's gear dropdown. Choose **Live** (6–18 seconds of reserve) or **Smooth** (12–24 seconds). While watching, right-click another channel and choose **Add as playback option** to approve a fallback. Remove the saved feed with the × beside its name under **Backup stream**. Click outside or press the gear again to close settings. Settings are saved per profile; backup mappings store IDs, not addresses or passwords, with a limit of 64 mappings.

Recovery state is fixed-size, with O(1) work per observation. Cooldown lookup scans at most 64 entries. Backup search scans the shared catalog only when its query changes and renders at most 20 matches. No standby stream runs. The existing 30-second read-ahead and 2 MiB queued-payload caps remain; these are not a total RAM limit. HTTP-date parsing reuses a package already in the dependency graph.

## Verification

Tests cover outages, frozen video, background suspension, pauses, retry budgets, gap limits, both buffer modes, backup/format loop prevention, profile isolation, missing and partial segments, expired links, split playlist headers, cooldown propagation, and existing conversion/seeking behavior. Native tests, lint checks, and release builds are run locally.

With `--logs`, recovery events record the cause, chosen route, buffer, and observed recovery time. Provider delivery logs separate upstream time from downstream backpressure.

**Still needs a real-service watch test.** The local app was not running and no browser was available during implementation. Local fixtures cannot prove seamless recovery, sound synchronization, or stable peak RAM against your provider. A buffered seek is a lightweight repair, not a full decoder rebuild with the connection preserved. A provider that stops sending all feeds will still interrupt playback.
