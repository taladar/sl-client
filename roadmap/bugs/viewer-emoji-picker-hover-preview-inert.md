---
id: viewer-emoji-picker-hover-preview-inert
title: Hovering an emoji never fills the picker's preview line
topic: viewer
status: bugs
origin: relief-theme live look in the gallery (2026-09-23)
refs: [viewer-emoji-picker-floater]
---

## Observation

The emoji picker's preview line reads "Hover an emoji to preview it" and
hovering an emoji **does nothing** — the line never changes. Confirmed by the
user in the real **floater** (opened from the gallery's floater switcher), not
only in the gallery's element card, so this is not the static specimen.

Worth stating because it misleads: the element card
(`spawn_emoji_picker_specimen`) really *is* inert by design — its tone row is
spawned "static — no press observers" and its preview line is plain prose from
the sweep sample. Only the floater is expected to be live, and it is not.

The picker is otherwise fine: it shows no visual glitches, and its emoji
render correctly.

## Where the wiring is

`sl-viewer-chat/src/emoji_picker.rs`, `spawn_emoji_cell` (~line 520). Each
cell gets two observers; the hover one is at ~line 572:

```text
.observe(
    move |_over: On<Pointer<Over>>,
          cells: Query<&EmojiCell>,
          state: Res<EmojiPickerState>,
          ui: Option<Res<EmojiPickerUi>>,
          mut texts: Query<&mut Text>| {
        let Ok(&EmojiCell { emoji, .. }) = cells.get(cell) else { return };
        let Some(emoji) = emoji else { return };
        if let Some(ui) = ui
            && let Ok(mut text) = texts.get_mut(ui.preview)
        { /* write preview_text(emoji, state.tone) */ }
    },
)
```

Every failure path in it is a silent `return`, which is why nothing is logged.

## Leads, in order

1. **The `Pointer<Over>` never reaches the cell.** The cell carries
   `Pickable::default()` and has a `glyph` child; if the child is the hit
   target and the event does not bubble to the cell, the observer never runs.
   The *press* observer on the same entity is reported working (clicking an
   emoji inserts it), which argues the entity is pickable — so check whether
   `Over` specifically is being consumed, or whether the grid's viewport /
   `VirtualList` scroll container absorbs it.
2. **`EmojiPickerUi` missing.** `Option<Res<..>>` makes a missing resource a
   silent no-op. It *is* inserted (line ~860), so this is the weaker lead, but
   it is worth confirming the resource is alive at the time the floater is
   open rather than assuming.
3. **`EmojiCell::emoji` is `None`.** A cell whose bound emoji was never filled
   in by `bind_emoji_rows` returns early. Would also make the press do
   nothing, so likewise weaker.

## What to add once fixed

A contract/interaction test that hovers a cell and asserts the preview line's
`Text` changed. The press path has coverage; the hover path evidently has
none, which is how a silent `return` chain stayed inert.
