# Full palette follow-up — 2026-10-31

The user rejected the accent-only picker from c156d31. The replacement changes canvas, panels, rows, hover, text, secondary text, borders, glass, accent, fill and selected surfaces together. Account has five named miniature surface/text previews and Reset to default. Charcoal is the default; stored Blue/Teal/Violet/Amber names remain valid. Unknown values fall back safely. Storage failures keep the session change and expose a status message.

## History and scope

A read-only history scout found the old eight-color control was for profile avatars (06ef02d, removed b84df32). The old global warm rose theme was fixed CSS at fcb3623's parent. No tracked global palette chooser predates c156d31. The new full palettes are deliberate expansions, not recovered historical choices. Profile identity and credentials are unchanged. Scrollbars and playback ranges remain neutral; video overlay text stays readable independently of the chosen app palette.

## MAST and checks

The implementation worker owned appearance.rs/theme.css; the coordinator owned mobile detail fades, integration and this ledger. A separate reviewer found no concrete blockers in token propagation, persistence/reset, controls or mobile geometry. No dependencies or polling were added.

Required ai-setup check-project passed. Release dx web build passed. Scoped Rust formatting and diff check passed. Two native tests extracted from the exact enum implementation passed (saved names/default and all 13 tokens); this does not imply the full wasm app test suite ran on the host.

Browser checks used an isolated app on loopback port 3212 and the repository mock provider on 3211 because the preexisting 3210 profile could not connect. At actual 320px width all five selections changed root background, menu panel, row and text values; every choice measured 52px tall and root horizontal overflow was absent. Blue survived reload into the profile chooser; Reset selected Charcoal. Violet propagated into the mobile sort sheet, detail background and facts card. At 1280px desktop its sidebar and navigation used the same panel, and Movies/Shows had no root overflow. Picker screenshot: /tmp/riptv-full-palettes.jpg.

Computed minimum contrast for text/dim/faint against canvas/panel/row: Charcoal 5.28, Blue 5.22, Teal 5.20, Violet 5.03, Amber 4.90. White on fill ranges 5.91–7.37. These token comparisons are not whole-app accessibility certification. Physical phones, native Firefox, storage-denial injection and actual playback were not rerun in this palette-only pass. Prior player geometry fixes remain intact; video styling received source review.
