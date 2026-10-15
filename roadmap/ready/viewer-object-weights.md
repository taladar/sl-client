---
id: viewer-object-weights
title: Object weights / land impact floater
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-object-selection-core]
refs: [api-g15, viewer-mesh-cost-estimate]
---

Context: [context/viewer.md](../context/viewer.md).

The object-weights display for the current selection: prim count vs **land
impact**, and the download / physics / server / display weight breakdown —
the numbers mesh builders watch constantly. Weights come from the selection
via the resource-cost surface ([[api-g15]]; `ObjectProperties` extended
fields carry pieces too). Live-updates as the selection changes.
Upload-time cost *prediction* is [[viewer-mesh-cost-estimate]]; this
floater shows the sim's authoritative numbers for existing objects.

Reference (Firestorm, read-only): `llfloaterobjectweights`,
`floater_object_weights.xml`.

Deps: [[viewer-object-selection-core]] (a selection to report on).

Audit 2026-10-15: the selection exists ([[viewer-object-selection-core]]
done), and the selected linkset's land impact is already fetched over
`GetObjectCost` through the shared `ObjectCostModel`
(`sl-viewer-world-objects/src/object_cost.rs`) and shown in the build
floater's info line (`sl-viewer-edit/src/edit_params.rs`) and the hover
tooltip. What remains is the weights floater itself: the download / physics
/ server / display breakdown and the region / parcel totals.
