---
id: viewer-inventory-search-filter
title: Inventory search / filter
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-inventory-ui
blocked_by: [viewer-inventory-folder-tree]
---

Context: [context/viewer.md](../context/viewer.md).

**Search / filter** over inventory: a text search plus filters by asset type,
date, and worn / recent, narrowing the folder tree
([[viewer-inventory-folder-tree]]) to the matching items. The filter drives
which nodes the tree (and gallery) show.

Builds on the folder tree's model; this task is the filter state and its
application to the presented item set.

Reference (Firestorm, read-only): `llinventoryfilter`,
`llinventoryfunctions`.

Builds on: [[viewer-inventory-folder-tree]].

## Done (2026-07-18)

The **name search** shipped in `sl-client-bevy-viewer/src/inventory.rs`: the
toolbar text field filters the shown rows, and on the Everything tab it keeps
the folder **hierarchy** that leads to each match (ancestor folders retained and
shown expanded), matching the reference viewer. Only loaded folders' items are
searchable.

The **type / date / worn-recent filters** were deliberately deferred — follow-up
[[viewer-inventory-advanced-filters]].
