---
id: viewer-outfit-editor
title: Outfit editor — edit the current outfit, save to My Outfits
topic: viewer
status: ready
origin: user request (2026-07-22); follow-up to
  viewer-inventory-cof-maintenance
refs: [viewer-inventory-cof-maintenance, viewer-inventory-replace-outfit]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **outfit editing** surface (Appearance floater / "Edit
Outfit"): the current outfit listed by category (attachments, clothing
per layer, body parts) from the COF links the viewer now maintains,
with add (from inventory) / take-off per row, **Save** / **Save As**
into a new outfit folder under My Outfits (links + copies the way the
reference composes outfit folders), wearing a saved outfit from the
list (via the Replace / Add paths that already exist), and the "now
wearing" summary. Layer re-ordering inside a type is its own task
([[viewer-outfit-layer-reorder]]).

Reference (Firestorm, read-only): `llpaneloutfitedit.cpp`,
`llpaneloutfitsinventory.cpp`, `llappearancemgr.cpp`
(`makeNewOutfitLinks`).
