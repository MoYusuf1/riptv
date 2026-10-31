# Palette and layout review — 2026-10-30

## Direction and evidence

Keep the media first: neutral dark surfaces, the existing locally bundled RIPTV Sans,
quiet selection/focus accents, rounded controls, and the three-type navigation pill.
The title stays above mobile search, with Sort alongside it. Scrollbars, volume and
movie seeking stay neutral independently of the chosen color.

Palette roles follow [Radix's background/component/text separation](https://www.radix-ui.com/colors/docs/palette-composition/understanding-the-scale).
The specific colors are a local design choice for this brief, not a claim that one
palette is universally best. White text on the four filled-action colors was calculated
against the [WCAG normal-text 4.5:1 criterion](https://www.w3.org/WAI/WCAG22/Understanding/contrast-minimum.html).
These checks do not certify every app state or the whole application.

| Choice | Accent for focus/selection | Filled action | White-on-fill contrast |
| --- | --- | --- | --- |
| Teal (default) | `#80cbc4` | `#176a63` | 6.41:1 |
| Blue | `#a8c7fa` | `#345fbe` | 5.96:1 |
| Violet | `#c4b5fd` | `#6c4ac2` | 6.18:1 |
| Amber | `#e7bd74` | `#80591e` | 6.24:1 |

Shared surfaces are background `#111113`, panel `#19191c`, row `#242427`, and
primary text `#f1f1f3`. The account menu exposes four named, pressed-state controls;
no numeric color sliders are needed. The finite enum accepts only known choices,
stores one browser-wide preference, and visibly reports a failed save as session-only.
Profile identity colors remain separate. No dependency, font service or polling was added.

## Concrete failures and corrections

- Standard scrollbar styling still supplied the old rose color while only WebKit
  thumbs had been overridden. Both now use shared neutral tokens.
- Selected states had hardcoded blue/rose values. Focus, selection and action fills
  now use the chosen palette; the seek progress and its focus ring remain neutral.
- Next page changed the catalog but left the phone's parent panel over 8,000 pixels
  down. Explicit paging now resets that panel and focuses the first new title.
  Search/sort updates do not request that focus, preserving typing.
- At 820px, two posters stretched to about 382×574px. The tablet grid now fits four
  186×279px posters, while narrow phones retain two columns and a 2:3 art ratio.
- Profile Back/source buttons measured 32/36px, and player controls measured 40px.
  Relevant form, clear-search, settings and player targets now have a 44px floor.
  The phone movie player's duplicate Download link stays on details, leaving six
  primary controls on one row; the series Next control retains an accessible name
  as a 44px icon on narrow phones. Landscape keeps its text and the spacer between
  control groups.
- Details advertised Back (Esc) without handling it. Detail, season and account
  dismissal now respect ordering and restore focus to a button, avoiding a search
  field that could summon a phone keyboard.
- Escape was swallowed by the native-input guard while Seek had focus. Escape now
  reaches the existing menu/fullscreen/player dismissal path; other range keys remain
  native. A visible thumb ring replaces the box around the full seek hit area.

## Browser acceptance

Use the existing MAST 0–4 rubric. This pass targets **3: coherent and verified**;
geometry/contrast measurements do not establish latency, device or heap quality.
Tests use a dedicated mock profile and local media fixtures, not real providers.
Viewport sizes below are measured CSS sizes; the browser's existing zoom was
accounted for rather than assuming the requested outer size was the CSS viewport.

| Layout / interaction | Evidence |
| --- | --- |
| Chooser and empty Add profile, 320×740 | No horizontal overflow; Back and source-type targets measure 44px. Returned without saving a profile. |
| Color menu, 320px and 390px | All four choices update selected state and navigation color; Teal survives reload. Choices measure 44px. Navbar interaction dismisses the menu. |
| Account menu, 740×360 | Menu height is bounded; keyboard focus reaches Quit after scrolling its content. Escape returns to Account. No Quit activation. |
| Live categories/channels, 320px | Category drill-down, title/search placement and neutral list scrollbar are contained. Local H.264/AC-3 selection opens a 320×180 player. |
| Live controls/settings, 320px | Seven 44px controls fit inside the 320px row, with the last ending at 316px. Both compact mode choices measure 44px. |
| Live short landscape, 740×360 | Player fills the viewport, control row has no horizontal overflow; mode-choice hit tests reach the modes themselves. Settings remain inside the viewport. |
| Movie catalog, 320/390/820/1280px | No root horizontal overflow. Poster art is about 139×208 / 168×252 / 186×279 / 139×209px; tablet has four columns, desktop six. |
| Paging, 390px | Repeated Next/Previous reset parent scroll to zero and focus the first title of the new page. |
| Empty search and Sort, 320px | Empty recovery works, clear target measures 44px; Sort contains only ordering choices. Rounded choices measure 44px in a contained 300px sheet. |
| Series detail, 1280px | Season 2 displays three episodes. One Escape closes the season menu and restores its trigger; account Escape closes only that menu, then closes details on the next Escape and restores the originating tile. |
| Movie detail, 320px | Settled detail bounds match the viewport; actions remain contained. Play opens the local movie fixture. |
| Movie player, 320px and short landscape | Movie retest passed: six 44px controls share one row, ArrowRight advances Seek by one native step, the thumb focus ring is visible, and Escape closes Watch to Play, then details to the originating tile. Decoded local video and neutral progress were checked. Series retest also passed: seven 44px controls share one row at 320px; Next advances to the next episode. At 740×360, controls stay on one row and Next keeps its text. |

## Validation and limits

The final required split project check and web release build both passed. The
movie and series player retests passed after that build. Owned Rust files pass the
scoped formatter check. The preliminary workspace-wide format check flags existing
`recovery.rs` formatting, which remains unchanged.

Independent source review covered persistence, constant-only CSS generation,
focus effects, menu ordering and native-key preservation. Physical touch, software
keyboards, notches, native Firefox rendering and real-provider long sessions remain
unverified. The mock fixture's metadata is not an authoritative duration benchmark.
