# Live playback: measurements and stream types

Measured on 2026-09-30 against a real Xtream account (UK/US/international provider), in Chrome
on Linux, with `cargo riptv --logs`. Times are from clicking a channel to the first decoded
picture and the first decoded sound (taken from the page; the proxy's timings come from
`/tmp/riptv-diagnostics.log`). Every channel was opened fresh.

## Before and after

"Before" is the standard player as of `594ed3a` (the browser's own HLS player, with ffmpeg as a
fallback chosen after a check); "after" is the `/live` pipeline from `1b945c8`
(`proxy/src/live.rs`).

| Channel | Stream | Before | After |
|---|---|---|---|
| BBC News | H.264 + AAC | sound at 4.0 s (muted until a polled frame check) | 1.7 s |
| Sky News | H.264 + E-AC-3 | picture only, never sound | 1.0 s |
| CNN | H.264 + AAC | switched away mid-play at ~7 s | 1.0 s |
| Al Jazeera | H.264 + AAC | browser decoder error a few seconds in | 1.0 s |
| MTV FHD 5.1 | H.264 + E-AC-3 | picture only, never sound | 2.0 s |
| Comedy Central FHD 5.1 | H.264 + E-AC-3 | picture only, then error | 1.6 s |
| Boomerang FHD 5.1 | H.264 + E-AC-3, segments start mid-GOP | picture only, never sound | 1.7 s |
| Sky Sports UHD 1 5.1 | HEVC 4K + E-AC-3 | 24 s | 2.7 s |
| Film4 HEVC HB | HEVC 1080p + AAC | 503 through the old path | 2.3 s |
| BBC One Channel Islands | H.264 1080i + AC-3, 1 s segments | — | 4.5 s |
| ABC (US) | H.264 + AAC | failed | 3.1 s (provider: 2.1 s to answer the playlist) |
| LBW: A&E | H.264 + AAC | failed | 1.5 s |
| SiriusXM (radio) | AAC, no video | not playable | 3.4 s (audio only) |
| beIN Sports (CA) | H.264 + AC-3 | — | 1.9 s |

Where the time went before: a separate request to learn the codecs, the browser's HLS player
buffering a whole segment, ffmpeg's own HLS reader re-fetching the playlist and probing for 3 s,
an ffprobe check in front of it, and a second connection that made the provider end the first.
Now there is one connection, one pass over the bytes (already on the provider's server, six
seconds back from the live edge), and the browser plays the first half-second fragment.

## Stream types seen, and how each is handled

| Stream | Handling |
|---|---|
| HLS → MPEG-TS, H.264 + AAC (most channels) | picture copied, sound re-encoded to AAC |
| H.264 + E-AC-3 (Sky Now 5.1, Sky News, TNT Sports) | picture copied, sound → AAC |
| H.264 + AC-3 (beIN, regional) | picture copied, sound → AAC |
| HEVC + AAC or E-AC-3 (4K, "HEVC" categories, 24/7 movies) | picture re-encoded (NVENC where it works), sound → AAC |
| H.264 interlaced (1080i UK regional) | deinterlaced and re-encoded, sound → AAC |
| Audio only, AAC (SiriusXM) | sound only; the player shows audio-only at once |
| Segments that don't begin on a keyframe | started at the first keyframe, with the PAT/PMT in front |
| A short text reply instead of video ("Cannot read …") | reported as the channel being down at the provider |
| HTTP 503 / 509 from the provider | reported as the provider failing / the connection limit |

Why sound is always re-encoded: copying AAC broke the browser's audio decoder on some channels
(`PIPELINE_ERROR_DECODE: Failed to send audio packet`), in ways the codec sniff can't see. AAC
encoding costs about 1% of a core.

## Found and fixed along the way

- Sound was muted until a polled frame check passed: seconds of silent picture.
- Any second request for a channel (a parallel codec check, the diagnostics probe) ended the
  player's session on providers that allow one connection per channel.
- Leaving a channel left the proxy following its playlist; the provider then answered 509 to
  the next channel (`leaving_a_live_channel_stops_reading_it` covers it).
- ffmpeg couldn't size the picture when a segment began mid-GOP ("dimensions not set").
- A 4K stream needed more than ffmpeg's half-second probe before its first sound.
- ffmpeg's `nobuffer` flag dropped every frame of an interlaced stream.

## Live buffering and recovery

The standard live player now starts from up to 18 seconds of available HLS history and
feeds converted fragments into an explicit MediaSource buffer, retaining up to 30 seconds
ahead and trimming old playback data. It waits for six seconds of browser buffer before
starting or resuming after an underrun.
Browsers that stop preloading sooner may resume with a smaller reserve after 15 seconds.
This trades some live delay for fewer interruptions; it cannot make sustained slow delivery
keep up with playback.

Temporary request failures are retried twice, serially, with backoff. A 509 refusal closes
the stale source so the player can reopen the original URL instead of polling the same
redirect for a minute. Partially delivered segments are never replayed into the decoder.
Idle reads are bounded, and non-advancing playlists use a 10–20 second inactivity threshold.
The player replenishes its reconnect budget after a minute of healthy playback.

With `--logs`, each segment records bytes, video duration, total elapsed time,
`downstream_wait_s` (waiting on the player/conversion pipeline), and `upstream_s` (the
remaining request/delivery time). Slow delivery with negligible downstream wait points
to the upstream/network path rather than local decoding. An HTTP 502 returned by RIPTV
is a gateway wrapper; inspect the recorded upstream status before attributing the cause.

The guide refreshes every five minutes. When active playback contradicts an "offline"
provider listing, the UI identifies the inconsistency instead of displaying it as an outage.

### Validation on 2026-10-01

NFL 02 (Steelers–Browns) reproduced 17 stalls totaling 129.3 seconds of waiting in a
roughly 5-minute-22-second baseline session, including a 70-second freeze during upstream
509 refusals. Video was copied and conversion CPU use was low, with negligible dropped
frames: expensive video encoding did not explain that failure.

The explicit-buffer player then completed a 15-minute session with no recorded stalls,
including a deliberate pause/resume check. Its reserve was about 21 seconds before the
pause and around 30 seconds afterward. Upstream delivery was much faster during this
second run, so this is functional validation, not a controlled claim that buffering cured
the provider's refusals. The measurements cannot distinguish provider congestion from
the network route without testing an alternate connection.

A fresh NFL 02 session on the final build subsequently slowed again: a 10-second segment
took 17.1 seconds upstream with effectively zero downstream wait. The reserve depleted,
and the player paused to rebuild six seconds before resuming. This confirms that sustained
delivery shortfalls still cause buffering; the clean 15-minute run is not a guarantee of
provider stability.

Local regression tests cover serial recovery from a temporary 503, prompt closure on 509,
and preserving a partial segment without replaying its bytes. A gateway test verifies
that the browser-facing 502 preserves an upstream 509 in the error details.

## Open

Tracked as GitHub issues #1–#5: provider-side start-up waits, long-watch stability, movies and
episodes through the same pipeline, the experimental player in its own module, and the 1080i
re-encode start-up.
