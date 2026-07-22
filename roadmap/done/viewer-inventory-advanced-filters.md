---
id: viewer-inventory-advanced-filters
title: Inventory advanced filters (type / date / worn)
topic: viewer
status: done
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

Shipped 2026-07-22: the reference finder floater
(`floater_inventory_view_finder.xml`) as an `inventory_filters` module —
thirteen type toggles + All/None, Worn only, Since login (against this
session's start), Newer/Older than a days+hours cutoff, and Reset —
folded into the tree flattening as a search-style narrowed view (pure
`ItemFilter::passes`, unit-tested). Not carried over from the
reference: the created-by filters, the links combo, the FS permission
filters and Only Coalesced.
