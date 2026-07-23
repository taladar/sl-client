---
id: viewer-inventory-long-names-wrap-overlap
title: Inventory rows with long names wrap to multiple lines and overlap
topic: viewer
status: bugs
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
