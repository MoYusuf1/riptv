Download your app below, then open it. Everything is included.

- **Windows:** `RIPTV-Windows.exe`
- **Linux:** `RIPTV-Linux` — allow the file to run first if asked.
- **Mac:** choose Apple Silicon or Intel, unzip, then open `RIPTV.app`.

Your browser opens automatically. No Rust or FFmpeg setup.

## What improved

**Movies start sooner and more of them play.** RIPTV's built-in movie player was updated. Matroska
(MKV) movies start about a third of a second sooner, and several kinds of MKV that used to fail
now play: sound packed the way mkvmerge does by default, older files with stripped headers, files
with no index, and files whose clock starts late. Progressive MKVs made by FFmpeg are no longer
mistaken for interlaced ones and re-encoded.

**Experimental player: live channels show a picture almost at once.** If you use the Experimental
player, a live channel now shows its first picture as soon as it arrives instead of after a whole
segment or keyframe group. (The standard player is unchanged.)

| Since 0.2.11 | Before | Now |
|---|---|---|
| Experimental live channel, first picture (1080p, 40 Mb/s) | 1.20 s | 0.29 s |
| Same on a slower 8 Mb/s link | 1.82 s | 0.46 s |
| Experimental live, raw TS channel | 2.03 s | 0.03 s |
| MKV movie start (server answering in 300 ms) | 1.34 s | 0.99 s |
| MKV with laced sound, stripped headers, or no index | would not play | plays |
| Title page while details load | partial page | a steady placeholder |
| Loading message | "Buffering video…" for everything | says whether it is starting, buffering, seeking or switching |

Also: the Live/Smooth switch is a compact two-way toggle. Every release's numbers:
[What changed, in numbers](https://github.com/MoYusuf1/riptv/blob/main/docs/changes.md).

Apps are unsigned, so your computer may show a security warning.
The checksums and FFmpeg source download are for verification, not for running the app.

[Help](https://github.com/MoYusuf1/riptv/blob/main/docs/downloads.md)
