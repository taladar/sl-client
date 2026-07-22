---
id: viewer-outfit-layer-reorder
title: Clothing layer re-ordering UI (and token read-back)
topic: viewer
status: blocked
origin: user request (2026-07-22); follow-up to the COF
  layer-ordering tokens
blocked_by: [viewer-outfit-editor]
refs: [viewer-inventory-cof-maintenance, viewer-bake-cof-layer-order]
---

Context: [context/viewer.md](../context/viewer.md).

Re-ordering same-type clothing layers: the outfit editor's up / down
arrows on a layer row, rewriting the affected COF links' ordering
tokens dense (the write / renumber helpers from
[[viewer-inventory-cof-maintenance]] already exist —
`cof_order_description` / the renumber pass) and re-baking. Also the
**read-back**: on login / COF reload, order the held wear set by the
links' tokens so a stack arranged in another viewer comes back in the
same order (today the in-session order is whatever arrived).

Reference (Firestorm, read-only): `llpaneloutfitedit.cpp` (ordering
arrows), `llappearancemgr.cpp` (`getWearableOrderingDescUpdates`).
