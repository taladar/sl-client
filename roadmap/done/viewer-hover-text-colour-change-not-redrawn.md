---
id: viewer-hover-text-colour-change-not-redrawn
title: A floating text that only changes colour keeps the old colour until its text changes
topic: viewer
status: done
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
tag) — was accepted everywhere except the last step that would show it.

## Cause (confirmed by reading the chain end to end)

The billboard mesh carries its colour in the **vertex colours**
(`build_tag_mesh_data` bakes each glyph's `TextColor` into the quad it emits),
so a colour change only reaches the screen when the mesh is rebuilt. The
rebuild is gated on the *layout*:

```text
name_tag_billboard.rs, build_tag_meshes' query filter:
    (Changed<TextLayoutInfo>, With<TagText>)
```

and a colour moves no glyph, so nothing upstream had any reason to fire:
`layout_tag_text` gates on the text, the block, the bounds, the hinting and
`ComputedTextBlock::needs_rerender`, and bevy_text's own
`detect_text_needs_rerender` watches the text and font, not `TextColor` (stock
Bevy re-reads colour at *extract* time every frame, so its pipeline never needs
a relayout for one — this renderer bakes it into a mesh instead). Changing the
string dirtied the layout, which is why the colour then "arrived" with it.

The halves upstream were sound, and stay untouched: `ObjectFloatingText` carries
`raw_color`, `to_content` derives the `TagContent`, `sync_object_hover_text`
compares whole `TagContent`s (colour included), and `sync_tag_spans` writes the
new `TextColor` into each span. `build_tag_meshes` then reads the colour *live*
off the span entities (`computed.entities()[section].entity`), so a rebuild
alone — with no re-shaping — is enough to show it.

## Fix

`sync_tag_spans` marks the tag's `TextLayoutInfo` changed when a span's colour
actually moved (`recoloured`, set only inside the existing compare-then-assign,
so an unchanged frame marks nothing). That is the same rebuild lever
`apply_name_tag_settings` already pulls for a bubble-opacity change, and it is
the cheap half of the work: `layout_tag_text` never gates on that flag, so the
text is not re-shaped, only the mesh re-poured. The chain is
`sync_tag_spans → layout_tag_text → build_tag_meshes` in one `PostUpdate` run,
so the rebuild lands the same frame.

`TagContent` is compare-then-assigned on both writers (`compose_name_tags` and
`sync_object_hover_text`), so `Changed<TagContent>` already means a real
difference — this cannot turn into a per-frame rebuild treadmill.

## Verify

Unit (`a_colour_only_change_marks_the_layout_for_a_rebuild`): a colour-only
`TagContent` change marks exactly one tag's `TextLayoutInfo` — the filter
`build_tag_meshes` is gated on — the spans carry the new colour, and both the
frame before and the frame after mark **nothing**, which is what says the
rebuild is one-shot rather than a treadmill.

Live (OpenSim, confirmed by the user): `llSetText("same text", <1,0,0>, 1.0)`
after `llSetText("same text", <1,1,1>, 1.0)` turns red without touching the
string. A name tag line colour (a chat / title colour change) rides the same
`TagContent` path and the same rebuild.
