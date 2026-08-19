---
id: viewer-ui-font-preference
title: User font scheme & size adjustment
topic: viewer
status: ideas
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-ui-skin-tokens, viewer-ui-text-font-family-selection,
       viewer-preferences-colors-skins-tab]
---

Context: [context/viewer.md](../context/viewer.md).

A user-facing UI font choice. Firestorm ships selectable font schemes
(`FSFontSettingsFile` — Open Sans, Deja Vu, and other bundled
`fonts/fonts*.xml` sets), a global point-size adjustment applied on
top of the scheme (`FSFontSizeAdjustment`), and chat line spacing
(`FSFontChatLineSpacingPixels`). Our font families are hard-picked in
the skin CSS: [[viewer-ui-text-font-family-selection]] (done) was
about generic families shadowing colour emoji, not user choice, and
only `ChatFontSize` and `UiScale` exist as size knobs.

Because the skin system is bevy_flair CSS with design tokens
([[viewer-ui-skin-tokens]]), the natural shape is a font-token layer:
a preference (on the colors & skins tab,
[[viewer-preferences-colors-skins-tab]]) that overrides the
skin-supplied font-family and base-size tokens from a curated set of
bundled font schemes, live-applied like the skin/theme switch.

Reference (Firestorm, read-only): `indra/newview/fsfloaterfonttest.cpp`,
`indra/newview/skins/default/xui/en/panel_preferences_UI.xml`
(FSFontSettingsFile, FSFontSizeAdjustment), `fonts/fonts*.xml`,
`indra/newview/llviewerwindow.cpp` (font settings load).
