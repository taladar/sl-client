---
id: viewer-object-context-menu
title: Object hover / context menu entries
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-object-selection
blocked_by: [viewer-object-selection-core, viewer-ui-radial-menu, viewer-ui-context-menu]
refs: [viewer-land-context-menu, viewer-attachment-context-menu, viewer-hud-context-menu, viewer-object-menu-reorder-when-implemented]
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
discrimination here — but the *entries* of the non-object targets are their own
tasks now: avatar is done ([[viewer-avatar-context-menu]]), land is
[[viewer-land-context-menu]], the worn-attachment pies are
[[viewer-attachment-context-menu]], HUDs are [[viewer-hud-context-menu]]. This
task owns the **object** pie (and the small muted-particle-source one). The pie
XMLs are shared by every skin (Vintage overrides none), so `default/xui/en/`
is authoritative.

Scope decision (2026-07-21): reproduce the **full reference entry set** at the
reference compass positions, with not-yet-implemented entries declared greyed
(the `UNIMPLEMENTED` condition pattern from `src/avatar_menu.rs`), and wire the
simple ones whose wire paths exist — **Take**, **Take Copy**, **Delete**
(Return), and Touch / Sit Here where the interaction paths are already there.
The rest (buy, pay, edit, wear/attach, the script / pathfinding / derender
tails) go live as their features land;
[[viewer-object-menu-reorder-when-implemented]] then re-lays the pie by
meaning.

Reference (Firestorm, read-only): `llselectmgr`, `lltoolpie` (what is under the
cursor and what may be done to it), `newview/llviewermenu.cpp` (the entry
handlers), and `newview/skins/default/xui/en/menu_pie_*.xml` as the entry
checklist — including which entry sits at which slice today.

Builds on: the `objects.rs` lifecycle and the pick / selection set from
[[viewer-object-selection-core]].

**Required by [[viewer-ui-radial-menu]] — pin every entry's position.** Each pie
built here (object / avatar / land / attachment) must ship a regression test
that pins **every action's address** (its path of compass points from the root
pie) against a committed table, in the shape of `pie_menu`'s
`every_action_keeps_its_declared_address`. A pie is muscle memory — an entry's
compass position must never move between commits. The test must fail if an entry
moves, so that moving one is a *deliberate* edit to the committed table and not
a silent side effect of a reorder by someone unaware of the angular-stability
rule. No pie ships here without its address table pinned.
