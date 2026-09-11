---
id: viewer-environment-land-day-cycle-edit
title: Edit a land's day cycle in place
topic: viewer
status: ready
origin: Split out of viewer-region-environment-panel (2026-09-11)
refs: [viewer-region-environment-panel, viewer-environment-day-cycle-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **Customize Day Cycle** button on the region / parcel
environment panel (`LLPanelEnvironmentInfo::onBtnEdit`): it opens the
day-cycle editor on the day cycle the *land* is running — which is not an
inventory item — and takes the editor's commit straight back to the panel,
which publishes it as an inline `day_cycle` rather than a `day_asset`
(`onEditCommitted` → `updateParcel(newday, …)`).

Both halves are missing here:

- [[viewer-environment-day-cycle-editor]] is inventory-scoped throughout.
  Its `EditedItem` names the item the cycle came from, Save writes the
  asset back onto that item, and Save As mints a new one. It needs a
  **land** editing context — the reference's `CONTEXT_REGION` /
  `CONTEXT_PARCEL` — in which the cycle has no item behind it, Save means
  "give it back to the panel", and Save As still mints an inventory item.
- [[viewer-region-environment-panel]] needs the button, and a commit signal
  to answer on (the reference's `setEditCommitSignal`). Everything else it
  needs is already there: the panel holds the land's full inline
  `EnvironmentSettings`, so the cycle to edit needs no fetch, and
  `EnvironmentUpdate::day_cycle` is already encoded.

Worth doing because the alternative route is clumsy: to change one keyframe
of a region's day you currently have to have the cycle in inventory, edit
it there and publish the asset — and a region running an inline cycle
(which is what OpenSim always serves, since it resolves `day_asset`
server-side) has no inventory item to start from at all.

Reference (Firestorm, read-only): `llpanelenvironment.cpp`
(`onBtnEdit`, `updateEditFloater`, `onEditCommitted`),
`llfloatereditextdaycycle.cpp` (`KEY_EDIT_CONTEXT`, `setEditDayCycle`,
`setEditCommitSignal`).
