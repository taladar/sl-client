---
id: viewer-environment-land-day-cycle-edit
title: Edit a land's day cycle in place
topic: viewer
status: done
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

## Done

**The editor's session grew a source, and that is the whole shape of it.**
`DaySession.item: EditedItem` became `source: DaySource`, which is either an
inventory item or a **land** — the reference's own split
(`LLFloaterEditExtDayCycle`'s `KEY_EDIT_CONTEXT`: `CONTEXT_INVENTORY` against
`CONTEXT_REGION` / `CONTEXT_PARCEL`). Everything that asked `session.item` a
question now asks `source`, and there are only three: may this be edited, where
does a Save As file its copy, and does a Save need the settings-asset store at
all.

That last one is the answer that matters. A land Save publishes the cycle
**inline** and never touches inventory, so a grid that cannot store a settings
asset takes away the land session's Save As and leaves its Save alone — and
that is exactly the grid where editing a region's inline cycle is the only way
to change it, since a simulator that resolves `day_asset` server-side never
sends one back.

**Save hands back; it does not publish.** The reference's `onEditCommitted`
goes straight to `updateParcel`. Here the commit (`LandDayCycleEdited`) lands in
the panel's **draft**, and Apply publishes it with the day length and offset in
one request. Two reasons: the panel owns the permission tests, the `?parcelid=`
scope and the Apply / Revert pair, and a second path to the capability would be
a second copy of all three — and a commit that went straight out would be the
one edit on that panel Revert could not undo. An authored cycle outranks a
picked `day_asset` in `publish_requests`, as `coroUpdateEnvironment` puts
`day_cycle` ahead of `day_asset`.

**A Save As from a land session keeps its context.** Following the copy is right
for an inventory session — the cycle on screen is now stored in the copy, so a
window still pointed at the original would write the edit into the wrong item —
and wrong for a land one, where it would silently turn Save from "hand this to
the panel" into "write this item".

**The open carries the cycle rather than fetching it.** The panel already holds
the land's whole `EnvironmentSettings`; a region running an inline cycle has no
asset id to fetch by in the first place. And a land with *no* environment —
which is what most parcels are, answered as an `is_default` map with no day
cycle — opens on the built-in default day, as the reference's `onBtnEdit` falls
through to `setEditDefaultDayCycle`. Opening an editor on nothing at all was the
obvious failure here and it is the common case, not the corner one.

**One window, one guard.** The editor is a singleton and already asked before
replacing unsaved work (`SettingsConfirmLoss`); that guard is now one function
over a `DayOpen` holding either kind of open, so a land open held for a
confirmation comes back as the land open it was.

The Settings-folder lookup a Save As needs moved out of the WindLight bulk
importer to the crate root (`settings_destination`), since two unrelated
surfaces now mint settings items into it.

## Verified

`cargo test --release -p sl-viewer-environment` — 92 green, including the three
new ones: a land session's Save survives a grid with no settings-asset store
while its Save As does not (and a read-only panel opens a read-only window), an
authored cycle publishes inline and outranks a picked asset, and a land with no
environment of its own is edited as the default day. The crate's scheduling
sweep covers the new open system.

Not verified live: the round trip — Customize Day Cycle, edit a keyframe, Save,
Apply — has not been driven against a grid. Worth doing on OpenSim, which is
where the inline-cycle case is the *only* case.
