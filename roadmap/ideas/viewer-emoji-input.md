---
id: viewer-emoji-input
title: Colon-based emoji input
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Emoji entry in text fields: type `:` to open an inline autocomplete of emoji
short-codes (`:smile:`), filter as you type, and insert the Unicode glyph; plus
a full emoji-picker floater. Requires an emoji short-code dictionary and correct
Unicode rendering in the chosen UI framework's text widgets.

Reference (Firestorm, read-only): `llemojihelper`, `llemojidictionary`,
`llpanelemojicomplete` (the inline `:`-completer), `llfloateremojipicker`.

Deps: [[viewer-ui-framework]] (text-input widgets + Unicode text rendering),
[[viewer-social-panels]] (chat / IM input, the primary consumer).
