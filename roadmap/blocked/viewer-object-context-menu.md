---
id: viewer-object-context-menu
title: Object hover / context menu entries
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-object-selection
blocked_by: [viewer-object-selection-core, viewer-ui-radial-menu, viewer-ui-context-menu]
---

Context: [context/viewer.md](../context/viewer.md).

The **entries** offered for the object under the cursor or the current selection
— touch, sit, edit, open, buy, take, and the rest — and the dispatch of each to
the operation behind it (edit opens [[viewer-object-edit-floater-shell]], and so
on). It reads the selection set and pick target from
[[viewer-object-selection-core]].

The **widgets** that display them are not this task: the radial presentation is
[[viewer-ui-radial-menu]], the line one is [[viewer-ui-context-menu]]. This task
supplies the entry tree; both render it, and which one a user gets is a
preference (the reference's `UsePieMenu`). Author the entries **once**, against
the shared entry model, rather than per widget — the two drifted apart upstream.

Two things this task owes the radial widget, which the line one does not care
about:

- **A compass position per entry**, not an order. [[viewer-ui-radial-menu]]
  holds angular stability as its core invariant; that is only meaningful if the
  entries actually name their positions. "Touch is north" is this task's
  decision to make and to keep stable across releases.
- **The Sit/Stand-style pairs** declared as autohide chains sharing one
  position.

The entry set is **per target**, not one menu: the reference has a distinct pie
per pick target — object, avatar (self / other), attachment (self / other),
land, and muted particle source (seven `menu_pie_*.xml` files). Model the target
discrimination here.

Reference (Firestorm, read-only): `llselectmgr`, `lltoolpie` (what is under the
cursor and what may be done to it), `newview/llviewermenu.cpp` (the entry
handlers), and `newview/skins/default/xui/en/menu_pie_*.xml` as the entry
checklist — including which entry sits at which slice today.

Builds on: the `objects.rs` lifecycle and the pick / selection set from
[[viewer-object-selection-core]].
