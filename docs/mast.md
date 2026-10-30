# MAST UI issue ledger and workflow

This is the continuing checklist for the MAST UI and performance work. The evidence baseline is
the [UI and performance review](ui-performance-review.md), especially its browser acceptance,
mobile refinement, reliability, and playback follow-up sections. “Browser checked” means the
specific flow and viewport recorded there passed; it is not device certification or a performance
benchmark.

## Issue ledger

| Area | Current outcome | Remaining evidence or follow-up |
| --- | --- | --- |
| Mobile look and navigation | Minimal dark palette, clear type hierarchy, plain header, prominent title above search, adjacent sort control, two poster columns, and a bottom pill with only Live/Movies/Shows are implemented. No catalog subheadings or category counts. Browser checks cover 320/390/410/820px and no horizontal overflow. | Test on physical phones, including safe areas, software keyboard resizing, and touch. Keep the nav limited to section switching. |
| Live categories and channels | Mobile Live opens categories, then channels, with Back to categories; search and selection work through the flow. Browser checks cover category → channel → moving mock stream, Back, All, empty recovery, and focus return. | Physical touch and keyboard checks remain. |
| Pins and category list | Pin targets are 44×44px, rows at least 56px, All/search stay fixed while rows scroll, and counts were removed. Browser checks cover pin/unpin, stable selection, and focus. | Recheck on a narrow physical device and with a long category list. |
| Sort, search, pagination | The button beside mobile search opens a compact sort sheet; pagination belongs to the scrolling catalog. Previous/Next targets are 44px. Browser checks cover dismissal, focus restoration, page changes, and 320px layout. | Check software-keyboard and touch behavior on device; collect a repeatable interaction-latency baseline. |
| Profile/account | Rounded account control and circular monogram are implemented. Outside-click dismissal uses a capture listener; Escape restores focus. Browser checks include navigation and search clicks that stop bubbling. | Check account geometry and dismissal on a physical phone/tablet. The saved profile list has no overall count cap. |
| Live settings and stream information | Live/Smooth options use readable rows and selected state; information uses labeled values/units, avoids guessed frame rate, and sampling stops on close. Browser checks cover 320px settings and short playback. | Hardware audio, long sessions, and repeated open/close resource behavior are unmeasured. |
| Live picture geometry | Missing guide/loading/error states reserve stable player geometry; at 1280×600 the populated/absent guide rectangles matched at about 626×318px. Portrait 320px and landscape playback layouts were checked in-browser. | Recheck short-height devices and actual browser fullscreen behavior, particularly iOS. |
| Movie startup and lifecycle | Guessed percentages were replaced by honest status and an indeterminate indicator. Playback launch/close restores focus and background inert state. The stall watchdog holds a weak movie reference; Watch fullscreen uses the shared cancellable controller and exits only its own captured element. | Browser checks cover fullscreen Escape ordering, repeated keyboard seeks, pointer commits, and synthetic seek cancellation. Real-device touch cancellation, audio, PiP, native fullscreen, and repeated-session cleanup remain unverified. |
| Saved state and series resume | My List retains its prior state on failed persistence, exposes an associated alert and recovers on retry. Successful shelf writes reuse decoded data. Episode resume and bars share identity/epoch caches. Tests cover failed writes, cross-tab shelf refresh and 10,000-episode lookup counts; browser checks cover resume/fallback/finish and injected failure recovery. | Cross-tab progress is not reactive; OS quota exhaustion and browser latency/heap measurements remain unverified. |
| Series season selector | The final narrow-viewport pass found 40px targets. The trigger and menu rows now have a 44px minimum; browser checks verify Season 2’s three episodes, focus return, Escape dismissal and no overflow at 320px. | Physical touch remains unverified. |
| Runtime and memory | Search/filter, sorting, page-bounded rendering, and proxy bounds are documented in the review. Category counts were removed. Controller transition tests and progress operation-count tests are documented there. | No repeatable browser latency, heap, image/GPU, or real-device peak-memory measurements exist. Per-title progress history and saved profile counts lack an overall cap. Component limits are not total-process memory guarantees. |

## Acceptance and evidence

Use the review’s 0–4 scale: **0** broken/unbounded, **1** fragile, **2** adequate with an evidenced
issue, **3** coherent, bounded and verified, **4** measured strong with repeatable evidence. Do not
award 4 without a repeatable measurement. Keep browser checks, native tests, device checks, and
measurements distinct in reports.

For each change, verify the complete interaction across component boundaries: launch, focus,
visibility, association, dismissal/back, repeated actions, and cleanup. Check keyboard behavior,
320–390px phones, tablet, and desktop as applicable. Confirm no new polling and report the exact
check command and result. The project check is required before claiming completion; browser
behavior still needs browser evidence where relevant.

## Working roles and scope

- **Coordinator:** owns integration, cross-component decisions, browser validation, and the issue
  ledger. Give each batch a named file owner and acceptance flow.
- **Luna scoped worker:** audits or implements only the assigned files. Read relevant code first;
  send a concise plan before multi-file work; run the project check and diff check; report exact
  files, selectors or handlers, and known unverified cases.
- **Terra reviewer:** independently reviews the concrete diff for behavior, lifecycle, accessibility,
  performance, and boundary cases. Review findings become ledger items until resolved or explicitly
  deferred.

Use exclusive file ownership while changes are in progress. Coordinate any ownership transfer before
editing overlapping files. Work directly on the current main checkout; do not create a PR or branch
unless requested. Workers do not commit; the coordinator commits and pushes reviewed, checked batches
to main under the user’s current authorization, matching the existing commit subject/body convention.
Stage only owned paths. Keep each change scoped and verify the diff before reporting. Preserve user changes and unrelated
uncommitted work.

Do not edit the externally owned `rstreamkit` repository, `Cargo.lock`, `app/Cargo.toml`, or
`app/src/playback.rs` unless ownership is explicitly reassigned. Do not access credentials, browser
storage, or session histories for this work. Keep the ledger evidence-based and omit secrets and
transcripts.
