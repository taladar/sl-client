---
id: viewer-inventory-advanced-filters
title: Inventory advanced filters (type / date / worn)
topic: viewer
status: ready
origin: split from viewer-inventory-search-filter (2026-07) — the name search
  shipped, the non-text filters did not
---

Context: [context/viewer.md](../context/viewer.md).

The **non-text filters** for the inventory window
([[viewer-inventory-search-filter]], done, shipped the name search): narrow the
tree by **asset / inventory type**, by **date** (since login, or a range), and
by **worn / recent**. Extends the existing filter state in `inventory.rs` (which
today holds only the name query) and the `build_rows` flatten.

Reference (Firestorm, read-only): `llinventoryfilter`.
