---
id: gridspec-inventory-fetch
title: Inventory skeleton, fetch caps, AIS3, the library and cache versions on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-ais3-nested-embedded,
  protocol-ais3-library-cap, protocol-fetch-inventory-items-request]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

SL drops the UDP descendents fetch silently (the fake grid refuses) and nests
AIS3 `_embedded` per depth; OpenSim's library leaves carry a phantom nil
folder.

Since done: the parser folds every nested `_embedded` level
(`gather_ais_embedded` in `sl-proto/src/session/conversions.rs`,
[[protocol-ais3-nested-embedded]]), and both runtimes read the agent tree over
`InventoryAPIv3` and the library over `LibraryAPIv3`, falling back to the
descendents caps and then UDP ([[protocol-ais3-library-cap]]);
`library-tree-fetch` and `inventory-tree-crawl` passed on aditi and OpenSim
on 2026-10-04. The fetch-item and cache-version cases on aditi, and recording
them into the book, are still this task's.

## Discover

Those cases on aditi plus an AIS3 depth probe and an item-fetch probe; skeleton
fields per option; whether versions advance on UDP / other-session changes.

## Document

`book/src/gridspec/inventory.md` § Fetching.

## Fake grid

Small — nested `_embedded` per flavour, OpenSim's phantom folder, version bumps.

## Viewer

No UDP on SL (the nested parser and `LibraryAPIv3` routing are done).
