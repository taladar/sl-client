---
id: viewer-emoji-data
title: Emoji dataset & lookup (adopt existing data)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-emoji-input
---

Context: [context/viewer.md](../context/viewer.md).

Adopt an existing emoji dataset behind a small shared lookup API — do **not**
hand-author one. The colon **short-codes** (`:smile:`) are a de-facto convention
(gemoji / emojibase), *not* part of the Unicode standard; the canonical
**names / keywords / groups** are Unicode **CLDR annotations** (reachable via
ICU / `icu4x`). The `emojis` Rust crate already bundles both (gemoji
short-codes + CLDR-derived names / keywords / groups + search) as a
self-contained dependency.

Scope: evaluate the `emojis` crate vs. pulling CLDR annotations directly, then
wrap the choice behind the two lookups the consumers need —
`short_code ↔ glyph` (for the inline `:`-completer) and a grouped/searchable
list (for the picker floater). Keep it a self-contained data module with no UI.

Consumed by [[viewer-emoji-colon-autocomplete]] (the inline `:`-completer) and
[[viewer-emoji-picker-floater]] (the full picker).

Reference (Firestorm, read-only): `llemojidictionary`.
