---
id: viewer-ui-text-emoji-presentation
title: Honour emoji-presentation (VS16) over a text font's own glyph
topic: viewer
status: ready
origin: gap surfaced by viewer-ui-text-foundation (2026-07)
refs: [viewer-ui-text-foundation, viewer-ui-text-font-family-selection]
---

Context: [context/viewer.md](../context/viewer.md).

A smaller gap found alongside [[viewer-ui-text-font-family-selection]]: a
codepoint with the **emoji-presentation selector** (`U+FE0F`, VS16) still
renders as the *text* glyph, monochrome, when the primary font happens to carry
one.

Concretely `❤️` (`U+2764 U+FE0F`) renders as a flat white heart: `parley`'s
`select_font` walks the primary family stack first, and the base text font
covers `U+2764` as a dingbat, so it wins before the `Emoji` generic (which holds
the bundled colour font) is consulted. Emoji with no text-font glyph — `🎉`,
`🔥`, `👨‍👩‍👧‍👦`, `🇯🇵` — are unaffected and paint in colour.

Per UTS #51 a VS16 sequence requests **emoji presentation** and should prefer an
emoji font even when a text glyph exists (and, symmetrically, `U+FE0E` VS15
requests text presentation and should prefer the text font). parley tracks
`is_variation_selector` in its `CharInfo` but its own source notes emoji
detection is incomplete ("to be used in more complete emoji checking, in
`select_font`"), and the selector does not currently reorder font selection.

Do: make VS16 clusters prefer the emoji family (and VS15 the text family) —
upstream in `parley::shape::select_font` if possible. Low priority: it affects
only the handful of dual-presentation codepoints, and the wrong result is a
legible monochrome glyph rather than a blank or tofu.
