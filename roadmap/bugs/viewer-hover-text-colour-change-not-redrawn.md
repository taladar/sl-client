---
id: viewer-hover-text-colour-change-not-redrawn
title: A floating text that only changes colour keeps the old colour until its text changes
topic: viewer
status: bugs
origin: user report while verifying viewer-nametags-refracted-by-distant-water (2026-09-10)
refs:
  - viewer-nametags-refracted-by-distant-water
  - sl-client-name-tag-billboard
---

Context: [context/viewer.md](../context/viewer.md).

Observed live (OpenSim): a prim's `llSetText` was set white, then re-set to red
with the **same string**. The text stayed white. Setting a *different string* in
the same change made the red appear.

So a colour-only update to floating text — and, by the same machinery, to a name
tag line (`TagContent` line colours, which is how chat and title colours reach a
tag) — is accepted everywhere except the last step that would show it.

## Cause (read, not yet measured)

The billboard mesh carries its colour in the **vertex colours**
(`build_tag_mesh_data` bakes each glyph's `TextColor` into the quad it emits),
so a colour change only reaches the screen when the mesh is rebuilt. The
rebuild is gated on the *layout*:

```text
name_tag_billboard.rs, build_tag_meshes' query filter:
    (Changed<TextLayoutInfo>, With<TagText>)
```

`TextLayoutInfo` is recomputed by `layout_tag_text` when the text block is dirty
— i.e. when the glyphs change. `sync_tag_spans` faithfully writes the new
`TextColor` into each `TextSpan` child (it compares and assigns), but a colour
does not change a single glyph, so `TextLayoutInfo` is untouched, the mesh is
never rebuilt, and the old vertex colours stay. Changing the string dirties the
layout, which is why the colour then "arrives" with it.

The wire and content halves look sound and should be confirmed rather than
assumed: `ObjectFloatingText` carries `raw_color` and derives `TagContent`
through `to_content`, `sync_object_hover_text` compares whole `TagContent`s
(colour included), so `TagContent` *does* change. The break is downstream of it.

## Fix sketch

Make the writer that knows a colour changed mark the layout dirty — in
`sync_tag_spans`, when a span's `TextColor` (or the root's `TagContent`
`base_color`) actually differs, `set_changed()` the tag's `TextLayoutInfo` (or
otherwise re-enter it into the `layout_tag_text` queue) so `build_tag_meshes`
picks it up. A colour-only change must not re-shape the text if that can be
avoided — shaping is the expensive half and the layout budget
(`SL_VIEWER_TAG_LAYOUT_BUDGET`) exists because of it — so the cheaper shape is a
rebuild path that re-pours the mesh from the *existing* `TextLayoutInfo`.

## Verify

Unit: a tag whose `TagContent` changes only in a line colour must end up with a
mesh whose vertex colours are the new ones (the existing `build_tag_mesh_data`
tests already reach the vertex colours).

Live: `llSetText("same text", <1,0,0>, 1.0)` after `llSetText("same text",
<1,1,1>, 1.0)` must turn red without touching the string; the same for a name
tag colour (a chat / title colour change).
