---
id: viewer-audit-about-land-row-rebuild
title: About Land rebuilds every owner and access row whenever any avatar name resolves
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-extract-and-test-pure-logic]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-places/src/about_land.rs:2065` (`sync_owners_view`) and `:2159`
(`sync_access_view`) both gate on `avatars.is_changed()` — which fires on
**every write to the name cache**, including a name for an avatar that appears
in neither list.

In a crowded region that is a full row rebuild, with a `translator.get()` and a
`format!` per row, many times a second while About Land is open.

Fix: gate on whether a name *this list displays* actually changed — compare the
resolved labels, or track a per-list revision — rather than on the whole
resource.

`about_land.rs` is 3309 lines with **zero tests**; several of its pure helpers
(`expiry_text`, `parcel_owner_label`, `day_cycle_summary`) are listed in
[[viewer-audit-extract-and-test-pure-logic]].
