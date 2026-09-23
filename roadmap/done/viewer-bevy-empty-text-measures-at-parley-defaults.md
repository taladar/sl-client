---
id: viewer-bevy-empty-text-measures-at-parley-defaults
title: An empty text node is measured at parley's defaults, not its own font
topic: viewer
status: done
origin: viewer-skin-checkbox-radio-shape, the checkbox tick (2026-09-22)
refs: [viewer-skin-checkbox-radio-shape, viewer-skin-glyphs-from-content]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

A `Text` node holding **no characters** is laid out as if it asked for
parley's default font rather than its own: a 20 px line, whatever
`TextFont` / `LineHeight` the entity carries. Measured in
`sl-viewer-ui-widgets/src/ui_checkbox.rs`'s layout test, a tick node with
`FontSize::Px(11.2)` and `LineHeight::Px(14.0)` reported `size 5×20`; adding a
single character to the same node reported `4×14`, the size it asked for.

## Why it is not cosmetic

The empty text node is the shape `content`-driven glyphs take: the widget
spawns a text host with `PseudoElementsSupport` and lets the skin write
`::before`'s `content` (`viewer-skin-glyphs-from-content` generalizes this to
icons). Those hosts sit inside small fixed boxes — a 14 px checkbox square, a
ring beside a 13 px caption — so a line six pixels too tall is content
overflowing its own box, and **taffy folds a child's content into every
ancestor's `content_size`**. Those six pixels reappeared as the checkbox
overflowing, then its row, then the Preferences tab panel, then the floater:
`every_element_fits_a_narrow_window`, `every_floater_fits_a_laptop_window`,
`every_floater_moves_with_its_title_bar` and
`every_resizable_floater_grows_with_its_grip` all failed at once, none of them
naming a font.

## Cause

`bevy_text`'s `pipeline.rs` pushes every span's style into parley as a
**ranged** property and skips ranges that are empty:

```rust
    if range.is_empty() {
        continue;
    }
```

With no text there is no range, so nothing about the node's font reaches the
builder and parley lays the empty line out at its own defaults (16 px at 1.2 →
19.2, ceiling 20). CSS says the opposite: an empty inline box still has the
line box of its own font.

## Fixed (2026-09-23)

In the `taladar/bevy` fork at `302316a`, on the branch this workspace already
pins: the first section's family, size, line height, letter spacing, weight,
width and style are pushed as the layout's **defaults** before the ranged
loop, so an empty block is measured at the style it declares. The ranged
pushes still win for every non-empty span, so a block that has text is laid
out exactly as before.

The two **zero-width spaces** that stood in for this are gone; `ui_checkbox`'s
tick and `ui_radio`'s pip spawn `Text::default()` again. The checkbox's
`the_tick_fits_the_box_it_sits_in` is what proves the fix landed: with the
workaround removed, that test fails the moment the measure goes back to
parley's defaults.

## A related fix that has landed

`bevy_flair` spawned a text `::before` as a bare `TextSpan`, so the
pseudo-element took bevy's default 20 px font instead of inheriting its host's
— the same 20 px by a second route. Fixed in the fork at `946f8a2` (`TextFont`,
`LineHeight` and `TextColor` are copied from the originating element, as CSS
says they inherit).
