---
id: viewer-key-object-contents
title: One Object Contents window per object
topic: viewer
status: done
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 3
refs: [viewer-keyed-floater-audit, viewer-profile-floater-single-instance]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-edit/src/edit_contents.rs`'s standalone **Object Contents** floater
(`"object-contents"`) is a singleton: `OpenObjectFloaterState::target` holds
*the* picked object, so opening a second object's contents repoints the one
window. Dragging an item from one object's contents into another's is a real
task, and it needs both listings on screen.

Convert it onto the keyed scaffold ([[viewer-profile-floater-single-instance]]).

## What moves

- **Key** by the object — `OpenObjectContents::full` (the grid-wide
  [`ObjectKey`]) rather than the region-scoped id, so the key survives what the
  scoped id does not. A `FloaterKey::subject`, so nothing persists per
  instance.
- `OpenObjectFloaterUi` (`panel` / `viewport` / `name_text`) and
  `OpenObjectFloaterState` become components on the window root.
- The per-surface split becomes per **window** for the floater half: today
  `ContentsViews`, `ContentsSelection` and `ContentsLastClick` each carry a
  `build` field and an `open` field, chosen by a `ContentsSurface` enum. The
  Build-tab surface stays a singleton (it is a tab in one window); the
  `open` half moves onto each floater instance. Expect `ContentsSurface` to
  survive as "which kind of surface a row belongs to" while *which instance*
  comes from the row's window (`host_floater`).
- Open through `KeyedFloaters::open`, build content at spawn, and order the
  open system `.after(FloaterSystems::Commands)` — the object pie's Open also
  raises whatever it was clicked through.

## What stays a resource

- `TaskInventoryCache` is keyed by object already and is shared by both
  surfaces — it is a cache of grid state, not window state, and must not be
  duplicated per window.
- `PendingMutations` tracks per-item optimistic state against the grid. Decide
  deliberately: it is keyed by item, and two windows on **different** objects
  cannot collide, so it can stay a resource — but say so in the module header
  rather than leaving it to be re-derived.

## The gotchas this one has

- **Row observers** (select, double-click Open, rename, remove) must resolve
  their window with `host_floater`; a double-click in one window must not act
  on the other's selection.
- The **double-click timer** is per surface today; per window it must not let a
  click in window A and a click in window B read as a double-click.
- The **permission gating** of New Script / Rename / Remove is per object, so
  it becomes per window: two objects can differ.

## How to verify

Open two objects' contents from the object menu: two windows, each listing its
own object with its own name line, count and selection; renaming in one must
not touch the other; closing one leaves the other. Pin the window count and the
independent selection with `instances` unit tests.

## Done (2026-11-04)

The Startup singleton, `OpenObjectFloaterUi`, `OpenObjectFloaterState` and the
`ContentsViews` / `ContentsSelection` / `ContentsLastClick` pairs are gone.
What one list shows and has picked is a `ContentsPane` (view, selection, last
click); the Build tab's is the `BuildContents` resource, and each window's
lives in a `ContentsWindow` component on the window root beside its target,
its latched object name and its widget handles. The window spawns through
`KeyedFloaters::open`, keyed by the object's `ObjectKey`, ordered after
`FloaterSystems::Commands`. A re-open raises the window, keeps its selection
and only refreshes its region-scoped id. How each gotcha came out:

- **Which window.** A `PaneRef` (Build tab, or a window entity) names the pane
  a row, a viewport or a button belongs to. Rows resolve it from their
  viewport; a window's buttons from `FloaterHost`; `ContentsActionRequest`
  carries it, so an action reads only its own pane's target, selection and
  permissions.
- **Double-click.** The timer lives in the pane, so a click in one window and
  a click in another can no longer make a double-click. The old timer was
  *one* resource shared by the Build tab and the floater, so this was a live
  cross-surface bug too, not only a keying concern.
- **Permissions.** Already per view; each window resolves its own object's. The
  window offers no permission-gated buttons (its Copy actions are the
  simulator's to arbitrate per item), so per window they gate its drop target.
- **The name.** The name line read the build selection's properties, which a
  keyed window would lose as soon as the selection moved to another object. A
  window now latches its object's name from the selection or from any
  `ObjectProperties` naming it, and puts it in the title bar once known, as
  About Land does with its parcel.
- **What stays a resource** — `TaskInventoryCache` (grid state, one listing
  and one fetch per object for every pane), `PendingMutations` (keyed by
  object and item; two views of one object should both show a change in
  flight) and `ContentsRename` (the Build tab's alone) — is said in the module
  header.
- **Reference.** `LLFloaterOpenObject` is a singleton over the current
  selection; keying it is a deliberate divergence, like About Land / Region.

Pinned by `edit_contents::tests::instances`: two windows under their own keys
and viewports, each listing its own object with one fetch per object, a
re-open keeping its selection and taking the new scoped id, closing one
leaving the other, an Open acting on its own window's selection and object, a
window's Copy button naming its own window, and clicks in two panes never
making a double-click. Live through the viewer by the e2e stage
`contents_window_per_object`
(`e2e_windows::each_object_lists_its_contents_in_a_window_of_its_own`): two
objects' pie Open slices open two windows each listing its own notecard, a
pick in one leaves the other's pick alone, a double-click opens the notecard
editor on the right object's item, and closing one window leaves the other.
