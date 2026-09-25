---
id: viewer-emoji-picker-hover-preview-inert
title: Hovering an emoji never fills the picker's preview line
topic: viewer
status: done
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

## A cheap discriminator (2026-09-23)

`viewer-skin-list-row-striping` found that `bevy_picking`'s `Hovered` is
**opt-in** and nothing was adding it, so every `:hover` rule on a non-`Button`
node painted nothing — `.sk-tile:hover`, the emoji cell's **highlight**,
among them. `skin::stamp_hover_state` now supplies it.

That is a *different mechanism* from this bug (the preview line is written by a
`Pointer<Over>` observer, which needs no component), but the two share an
input: both the highlight and the `Over` event come from the **`HoverMap`**. So
the fix turns the picker into a one-look experiment:

- the cell now **highlights** under the pointer but the preview is still
  empty → the cell is in the `HoverMap`, the event reaches it, and the fault is
  in the observer body or its resource lookups (leads 2 and 3);
- the cell **still does not highlight** → the cell is not in the `HoverMap` at
  all and lead 1 is confirmed, which also predicts `Pointer<Out>` never fires.

Worth running before touching any of the leads below, because it splits them in
half for the cost of opening the floater.

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

## Cause and fix (2026-09-26)

None of the three leads. The live viewer's picker previewed all along; a
headless test with `EmojiPickerPlugin` hovering a real cell passes before and
after the fix. The dead preview was the **gallery's**: on 2026-09-23 its
"floater" was a static specimen (the "Hover an emoji…" prose), and since
`viewer-gallery-floaters-are-mostly-stubs` it is the live content built by
`spawn_emoji_picker_specimen` in a host that runs no picker plugin. The hover
observer read `EmojiPickerState` and `EmojiPickerUi` as optional resources and
returned silently without them. So did the tone swatches, and the gallery grid
never filled a row past the ones the specimen drew.

The fix makes each grid its own picker and the gallery a host for it:

- `EmojiPickerState` / `EmojiPickerView` are components on the grid's
  viewport, beside a new `EmojiGrid` naming its search field, category strip
  and preview line (was: global resources, and `EmojiPickerUi` carrying the
  parts). Cells and tone swatches capture their grid; every system iterates
  grids.
- The grid systems (search, tabs, view rebuild, row populate / bind, tone
  highlight) are a new `EmojiGridPlugin`; `EmojiPickerPlugin` keeps the
  floater, toggle, anchoring and insert target, and adds the grid plugin.
- The gallery adds `EmojiGridPlugin`, so its Emoji floater searches, switches
  groups, takes a tone, previews and scrolls. Only inserting stays out: the
  gallery has no target field. Populate leaves alone a row that already has its
  cells, so the specimen's pre-drawn rows are adopted, not dressed twice.

Tests (synthetic pointer, `sl_viewer_testkit::interact`):
`hovering_a_cell_in_the_floater_previews_it` (full plugin),
`hovering_a_cell_in_the_specimen_previews_it` (no plugin, the sweep host; aims
at a glyph other than the pre-filled first one; failed before the fix) and
`the_specimen_under_the_grid_plugin_searches_and_takes_a_tone` (the gallery's
host: type `wave`, click the dark swatch, assert the checked swatch, the
re-cast cell and the toned preview).

## Closed (2026-09-26)

The user checked the gallery's Emoji floater: the preview follows the pointer,
and a tone swatch re-casts the waving hand. Only People & Body carries
tone-bearing emoji (330 of 388; every other group has none), so a swatch on
the opening Smileys tab changes nothing visible but the checked swatch — that
is the data, not a fault.
