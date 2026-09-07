---
id: viewer-key-object-contents
title: One Object Contents window per object
topic: viewer
status: ready
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
