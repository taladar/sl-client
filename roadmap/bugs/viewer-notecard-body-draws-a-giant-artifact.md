---
id: viewer-notecard-body-draws-a-giant-artifact
title: The notecard editor and reader draw a huge stray shape beside the body
topic: viewer
status: bugs
origin: relief-theme live look in the gallery (2026-09-23)
refs: [viewer-notecard-editor]
---

## Observation

In the gallery, the `notecard-editor` and `notecard-reader` elements each draw
a large stray shape to the **right of the body**, hundreds of pixels across:

- **`notecard-editor`** — a clean **white** rectangle, tall and narrow,
  spanning most of the body's height.
- **`notecard-reader`** — a **grey** region with a dark letter-like form in it
  (read as a giant "Z"), with an irregular, staircase-ish outline.

The body text itself renders correctly in both, embedded item included:
"Welcome! See <https://example.com> / or drop by 📍 Our Home" is laid out and
linkified as expected. The artifact is *additional*.

Reproduced by the user in **every skin and every theme**, so it is not a skin
rule.

## What it is NOT (each checked, each wrong)

- **Not the embedded-item marker's `.notdef` glyph.** `populate_editor` does
  hand the rich-text field the notecard's raw text including the
  `U+100000` embedded-item marker (`sl_notecard::embedded_char(0)`,
  Plane 16 private use), and nothing substitutes it — `shown_text` is only a
  dirty-check cache, not a display transform. That is worth fixing on its own
  (the reference replaces the marker with the widget and reserves its advance)
  but it is not this: a `.notdef` box is glyph-sized, not 400 px.
- **Not an `ImageNode`.** There is no `ImageNode` anywhere in
  `edit_notecard.rs`, `notecard_render.rs` or `ui_rich_text.rs`, so it is not
  a missing texture drawing as bevy's default white 1x1.
- **Not a wrongly sized rich-text overlay object.** `place_rich_text_objects`
  only ever writes `left` / `top` and `Visibility`; it never sets a width or
  height, so an object box cannot be stretched by it.
- **Not the emoji atlas.** The emoji picker, which is full of colour emoji,
  renders cleanly.

## The remaining lead

The staircase outline of the reader's shape is the geometry of a **multi-line
text selection highlight** — a partial first line, full middle lines, a
partial last one — and the two colours fit the two selection tokens: the
editor is editable (focused `selection-color`, near-white) and the reader is
read-only (`unfocused-selection-color`, grey). A spurious selection spanning
to the end of the buffer would look exactly like this. Unverified.

## Why it is not already pinned by a test

A headless probe that spawns both elements through `spawn_element` and dumps
the laid-out tree shows **nothing near that size** — the body collapses to
360x16 there and no node exceeds it. So whatever draws it is not in the layout
tree the harness builds, and reproducing it needs the real app (fonts, text
layout, selection state) rather than the layout stub.
