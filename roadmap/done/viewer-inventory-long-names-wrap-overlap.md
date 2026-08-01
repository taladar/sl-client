---
id: viewer-inventory-long-names-wrap-overlap
title: Inventory rows with long names wrap to multiple lines and overlap
topic: viewer
status: done
origin: user report during the R23/R25 aditi verification session (2026-07-23)
refs: [viewer-text-node-padding-measure]
---

Context: [context/viewer.md](../context/viewer.md).

An inventory item whose name is longer than the row is wide renders as a
**multi-line** label instead of a single line with the tail hidden, and the
extra lines **overlap the rows above and below** (the virtual list presumably
sizes rows at one line).

Expected (reference behaviour): a row label is single-line, clipped at the
row's width — ideally with an **ellipsis** (`…`) marking the cut, as the
reference viewer's `LLFolderViewItem` draws long names.

Suspects / shape of the fix:

- Force the row label single-line (no wrap) and clip it at the row bounds
  (`overflow` / the `TextMayClip` exception), so the overlap disappears even
  before ellipsis lands.
- Ellipsis proper needs a measure-and-truncate pass (`bevy_text` has no
  native `text-overflow: ellipsis`): truncate the string to the advance
  width that fits and append `…` — or an upstream contribution.
- Check the other virtual-list consumers (people list, group list, chat
  sessions) for the same wrap-overlap once the fix exists.

## Resolution

Both halves landed.

**No wrap + clip.** Each row's label now sits in a `label_clip_node` container
(`min_width: 0`, no `flex_grow`, `overflow: clip`) with a `TextLayout::no_wrap`
`Text` child (`flex_shrink: 0`, so the clip is what shrinks), so a name wider
than the row draws on a single line with its tail clipped instead of wrapping
into the neighbouring rows. The `Text` stays a bare child (the clip and shrink
live on the container) to dodge the [[viewer-text-node-padding-measure]] measure
loss.

**Ellipsis, the same way the tables do it.** Rather than a bespoke
measure-and-truncate, the row reuses the codebase's existing ellipsis mechanism
(`ui_table`'s `apply_table_cell_ellipsis`): a trailing `…` marker node between
the clipped label and the decoration, hidden by default, carrying
`i18n::LocaleEllipsisMarker` so its glyph is the **localised** `ui-ellipsis`
(`apply_locale_ellipsis` sets it). `apply_inventory_row_ellipsis` (in
`PostUpdate` after layout) reveals the marker exactly when the label overflows
its clip — the pure `ellipsis_visible` test, `content_size.x > size.x`. No text
shaping, no truncation of the string, no feedback loop. `bevy_text` still has no
native `text-overflow: ellipsis`, so this is done downstream rather than
upstream.

Both halves are guarded headlessly by
`a_long_inventory_row_label_clips_and_flags_the_ellipsis`: the long name comes
out single-line (self-calibrated against a plain wrapping column, so the test
has teeth) and its clip reports the overflow that reveals the marker, while a
short name reports none.

**Sibling lists.** The people and group lists build their name rows through
`ui_table`'s `spawn_table_row`, whose name cell already uses this exact
mechanism, so they never had this bug. The conversations *tab* strip labels
(`conversations.rs`) are `flex_grow: 1` / `min_width: 0` but lack `no_wrap`;
that is a distinct (tab, not virtual-list) widget and was left untouched.
