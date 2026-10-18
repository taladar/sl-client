---
id: viewer-terraform-parcel-refusal
title: A brush over a parcel the agent may not terraform warns nothing
topic: viewer
status: bugs
origin: gridspec-terrain-editing (2026-10-18)
refs: [gridspec-terrain-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's `LLToolBrushLand::handleMouseDown` raises
`ParcelNoTerraforming` (and still sends the strokes) when the parcel under
the cursor is not one the agent may terraform: it manages the estate, the
parcel's owner is the region's owner, or the agent owns it (or its group, with
`GP_LAND_ALLOW_EDIT_LAND`). Neither grid answers a refused stroke with
anything (`book/src/gridspec/terrain.md` § Editing), so without the check a
resident brushing someone else's land sees nothing happen and is never told
why.

Our Land tool has the region-level refusal (`RegionNoTerraforming`) but not
this one: it needs the hover parcel the reference keeps
(`LLViewerParcelMgr::getHoverParcel`, a `ParcelPropertiesRequest` for the
point under the cursor while the Land tool is up), which the viewer does not
ask for. Add the hover request, then the check on press.
