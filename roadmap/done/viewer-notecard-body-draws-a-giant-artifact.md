---
id: viewer-notecard-body-draws-a-giant-artifact
title: The notecard editor and reader draw a huge stray shape beside the body
topic: viewer
status: done
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

## Found, and fixed (2026-09-24)

**A hidden range's glyph.** `RichTextStyle::Hidden` — the mechanism that takes
an embedded item's marker code point off the screen, leaving the box drawn for
it — was `StyleProperty::FontSize(0.0)` and nothing else, on the documented
reasoning that a zero-size glyph has "no advance and no glyph". Only the first
half is true. The glyph is still emitted, and what its quad samples at a
degenerate size is the rasteriser's business: it came out as a few hundred
pixels of **raw glyph atlas** — an empty region in the editor (the clean white
rectangle) and a region carrying glyph shapes in the reader (the grey field, the
"giant Z", the staircase edge, which is the outline of a row of cached glyphs).

Hidden ranges now carry **two** properties: the zero font size, which is what
takes the advance away, and a transparent brush, which is what takes the glyph
away. Every field grows one more section for it, after its classes, at
`hidden_section_index`. *Hidden* means invisible on its own terms rather than
resting on what a rasteriser does at size zero.

### What the diagnosis actually turned on

Three measurements, in this order, and the middle one is the reusable part:

1. **The two "not an `ImageNode`" and "not a stretched object" notes above were
   right, and so was "not the marker's `.notdef`"** — but all three ruled out
   the wrong kind of thing. The shape is not a node at all.
2. **A dump of the live card's laid-out subtree** (`SL_GALLERY_DUMP_ELEMENT`,
   added for this) shows the card at 760x453, the field at 740x340, the embedded
   item at 103x19, the link at 146x19, and nothing else non-zero. A few hundred
   pixels of *anything* with no node to hold it means the text pipeline drew it
   — which leaves exactly two candidates, since `TextLayoutInfo`'s glyphs and
   its `selection_rects` are the only things `bevy_ui_render` draws without a
   node.
3. **`Hidden` is the only thing in the viewer that sets a zero font size, and
   its only live consumer is the notecard** — editor and reader, which is
   exactly the two elements affected and why the script editor, which shares the
   whole rich-text stack otherwise, is clean. That correlation is what chose
   between the two candidates without needing to reproduce either.

### The probe, which stays

`SL_GALLERY_DUMP_ELEMENT=<element-id>` dumps that gallery card's laid-out
subtree once — depth-indented names (so the indent is the ancestry), each box
and centre in logical pixels, and what each node draws (text with a char count,
an `ImageNode` with its tint, a non-transparent `BackgroundColor`). Element
cards carry a `Name` now (`gallery-card:<id>`) so there is something to address.
`SL_GALLERY_DUMP_AFTER=<frames>` moves the settle delay.

Two things it had to get right, both found the hard way:

- **`UiGlobalTransform`, not `GlobalTransform`.** A `bevy_ui` node carries the
  former; a query for the latter matches nothing and reports every node in the
  tree as "not a laid-out node", which reads exactly like a tree that never laid
  out.
- **An unfocused window stops running frames.** `bevy_winit` throttles to
  react-on-event, and a dump run never takes focus — the settle countdown
  stopped dead at 90 frames left and the dump simply never happened, twice,
  silently. A pending dump now forces `UpdateMode::Continuous`, and the
  countdown logs its progress so "still settling" and "never scheduled" stop
  looking the same.

### The check

`a_hidden_range_is_drawn_in_a_transparent_section` pins the mechanism rather
than the picture: the field's section list is field + one per class + a
transparent one, at the index a hidden range's brush is given. The *picture*
cannot be pinned here — nothing about the layout is wrong in the broken state,
which is the whole reason this bug survived a geometric harness.
