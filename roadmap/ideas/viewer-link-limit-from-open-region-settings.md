---
id: viewer-link-limit-from-open-region-settings
title: Take the linkset prim limit from the region where the region states one
topic: viewer
status: ideas
origin: gridspec-object-link-delink (2026-10-11)
refs: [gridspec-object-link-delink, viewer-prim-linking]
---

Context: [context/viewer.md](../context/viewer.md).

The Build tool refuses a link of more than 256 prims itself
(`MAX_LINKSET_PRIMS`, `sl-viewer-edit/src/edit_link.rs`), as the reference
viewer does on a grid that states nothing else. Second Life refuses the
257th prim too (`object-link-delink`, 2026-10-11), so nothing is lost
there. A stock OpenSim region links any number — a set of 257 was measured —
and states no limit, so there our viewer, like Firestorm, stops a link the
grid would take.

Firestorm reads the limit from the `OpenRegionSettings` /
`DispatchOpenRegionSettings` capability where a grid has one
(`MaxLinkCount`, and `MaxLinkCountPhys` for a physical set; zero meaning no
limit), and falls back to 256. We decode that record
(`sl_proto::OpenRegionInfo::max_link_count`) and the link tool does not
read it.

Wanted: the link tool's limit is the region's where the region states one,
256 otherwise. Whether to lift the limit altogether on a grid whose
simulator features say it is OpenSim and that states none is a decision —
it departs from the reference, and the grid is the final judge either way.
