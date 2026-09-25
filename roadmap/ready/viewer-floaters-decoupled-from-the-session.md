---
id: viewer-floaters-decoupled-from-the-session
title: Floater UI behaviour is welded to the session, so it runs nowhere else — not the gallery, not a unit test
topic: viewer
status: ready
origin: viewer-gallery-floaters-are-mostly-stubs — the user clicking through the new specimens (2026-09-25)
points: 13
refs: [viewer-gallery-floaters-are-mostly-stubs, viewer-ui-styling-interaction-tests]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Every gallery floater now shows its real content, built by the live window's
own builder and filled with sample data. But it is built **once**: whatever the
live window does *afterwards* happens in its plugin's systems, and those read
session resources (`SlEvent`s, the grid replies, the models the session keeps),
so the gallery cannot install them. In the gallery, then:

- **Search** shows result rows, but clicking another row does not switch the
  details pane — the selection → subject → detail redraw is a system;
- scrolling a **virtual list** (the emoji grid, object contents, the tables)
  moves the offset but never rebinds the rows to the items scrolled to — the
  row binds are the features' systems;
- tab switches that change what a pane shows, hover previews, status lines
  that follow state: anything reactive that is not an observer is inert.

The same wall stops a test: a floater's behaviour can only be exercised by
standing up a session, or the fake grid, around it.

## What to do

Put a seam between a floater's **UI** and the **session**:

- Each feature's window reads and writes a UI-level state it owns (a view
  model: the rows, the selection, what the detail pane shows, what is
  loading) and emits UI-level intents (select, open, request details), rather
  than reading `SlEvent`s and session models inside its view systems.
- The session side becomes an adapter per feature: session events → state
  updates, intents → session commands. That half is where the protocol lives,
  and it stays testable on its own.
- The view systems then run anywhere the state exists. The gallery installs
  each feature's **view** plugin and its specimen inserts sample state; the
  window behaves live — selection, scrolling, rebinding, detail panes —
  through the same systems the viewer runs.
- A unit test can drive a floater by writing state and reading intents, with
  no session and no fake grid.

Do it feature by feature. The first worth doing are the ones where a session
is the most expensive way to reach the behaviour:

- **Search**, and the virtual lists generally — their inertness in the gallery
  is the most visible;
- the **Friends and Groups lists** — presence, rights, sorting, the rights
  columns, the group roster: all of it today needs real friends and groups on
  a real grid to exercise;
- **Inventory** — the tree, its filters, folder operations, drag and drop,
  worn / link state: a large surface whose states (a deep library, a broken
  link, a no-copy item) are hard to arrange on a grid on demand;
- **About Land and Region / Estate** — much of these windows exists only for
  someone with special rights on the grid (a parcel owner, a group officer,
  an estate manager or owner). With the seam, a test or the gallery can put
  the window in "you own this parcel" or "you manage this estate" by setting
  state, and check every rights-gated control, instead of needing an account
  that holds those rights on a grid.

## Done when

Every gallery floater reacts to clicks, scrolls and tab switches as the live
window does, through the live systems, and at least the first converted
features have tests that exercise their behaviour without a session.
