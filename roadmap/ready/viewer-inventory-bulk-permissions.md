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

## Parity-audit addendum (2026-08-19)

Firestorm's `llfloaterbulkpermission` operates over the *build
selection's task inventories* — every item inside every selected
object's contents — not (only) over inventory folders as this task's
body describes. Support that selection-driven mode, and add its launch
point: the build floater Content tab's "Permissions…" button
(`floater_tools.xml` L3321), which opens the bulk-permissions floater
pre-scoped to the current selection.
