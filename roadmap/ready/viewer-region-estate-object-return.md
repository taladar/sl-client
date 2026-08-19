---
id: viewer-region-estate-object-return
title: Region Debug tab — estate-wide object return by resident
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-region-options-debug, viewer-god-tools]
---

Context: [context/viewer.md](../context/viewer.md).

The reference Region/Estate Debug tab returns all objects owned by a
chosen resident (avatar picker + "Return" button,
LLPanelRegionDebugInfo::onClickReturn) with three scope checkboxes:
include objects with scripts, only objects on someone else's land, and
apply to every region of the estate.

Our Debug tab (`sl-client-bevy-viewer/src/about_region.rs`,
[[viewer-region-options-debug]] done) has the
scripts/collisions/physics toggles plus restart/cancel-restart only —
no return UI in `AboutRegionAction`. The wire path exists unused:
`Command::SimWideDeletes` in `sl-proto/src/command.rs` carries exactly
the target-agent + flags shape those checkboxes map to. Implementing
this means a target-resident row (reusing the avatar picker), the three
checkboxes, a confirmation dialog, and the `SimWideDeletes` send,
estate-owner/manager gated like the rest of the tab.

Reference (Firestorm, read-only):
`indra/newview/llfloaterregioninfo.cpp` (LLPanelRegionDebugInfo),
`indra/newview/skins/default/xui/en/panel_region_debug.xml`.
