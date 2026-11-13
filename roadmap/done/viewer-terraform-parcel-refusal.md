---
id: viewer-terraform-parcel-refusal
title: A brush over a parcel the agent may not terraform warns nothing
topic: viewer
status: done
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

## Resolution

The Land tool keeps a hover parcel while a brush is picked
(`request_hover_parcel` in `sl-viewer-edit/src/edit_land.rs`): a
`ParcelPropertiesRequest` for the 4 m square under the (whole-metre rounded)
cursor whenever the move crosses a parcel line on the overlay, walked as the
reference's `setHoverParcel` walks it, with its own sequence ids counting down
from the reference's `HOVERED_PARCEL_SEQ_ID` (-50000). A brush press then
checks `canTerraformParcel`'s rule — estate manager, the region owner's land,
or land the agent owns itself, as a god, or through a group granting
`LAND_ALLOW_EDIT_LAND` — and raises `ParcelNoTerraforming` naming the parcel
when it fails, still sending the strokes. To answer the group half the
`GroupsModel` now keeps each membership's powers (`has_power`, the
reference's `hasPowerInGroup`), and `AgentGodLevel` moved from
`sl-viewer-notices` to `sl-viewer-social` so the edit crate can read it.
Unit tests pin the line walk, the request/answer matching and the rights.

`e2e_terrain`'s `a_brush_on_a_strangers_parcel_warns_on_{second_life,opensim}`
split the fake region between Alpha and a stranger and press the Revert
brush on each half: Alpha's own land strokes without a word, the stranger's
raises the warning naming the parcel and strokes all the same.
`a_brush_beside_a_resident_on_second_life_warns` presses beside the avatar on
aditi, where the Mauve public sandbox warns on both backends.
