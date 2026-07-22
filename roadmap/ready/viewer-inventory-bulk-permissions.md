---
id: viewer-inventory-bulk-permissions
title: Bulk next-owner permissions editor
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The bulk-permissions floater: select folders / items, choose next-owner
copy / modify / transfer (and the type filter — apply only to e.g. textures,
objects, clothing), preview how many items will change, then apply the
permission update across the selection with progress. Uses the existing
item-update path (`UpdateInventoryItem` / AIS3), skipping items the agent
cannot change (no-modify sub-items) and reporting the skips.

Reference (Firestorm, read-only): `llfloaterbulkpermission`,
`floater_bulk_perms.xml`.

Builds on: the held inventory model + item mutation (`protocol-30`), the
`sl-types` permission bitflags (`idiomatic-p1-01`).
