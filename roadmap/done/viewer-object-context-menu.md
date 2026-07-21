---
id: viewer-object-context-menu
title: Object hover / context menu entries
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-object-selection
blocked_by: [viewer-ui-radial-menu, viewer-ui-context-menu]
refs: [viewer-object-selection-core, viewer-land-context-menu, viewer-attachment-context-menu, viewer-hud-context-menu, viewer-object-menu-reorder-when-implemented, viewer-object-pie-buy-take-chain, viewer-object-pie-multi-select-take, viewer-object-pie-enable-fidelity, viewer-particle-pick-mute, viewer-flexi-prim-picking]
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

## Done (2026-07-21)

Landed as `src/object_menu.rs`: the full `menu_pie_object.xml` tree at the
reference compass positions (reference slice order → East..SouthEast, the
avatar-pie convention), opened as a **pie** (line presentation deferred, as
for the avatar pies). The whole tree's address table is pinned in
`object_pie_keeps_every_address`.

**How a pick resolves without the selection core.** The task was formally
blocked on [[viewer-object-selection-core]], but the menu needs only a
single-object pick, not the maintained selection set: the shared right-click
resolver in `src/avatar_menu.rs` now resolves the world ray against **both**
the mesh-accurate avatar pick and a first-hit object pick
(`object_menu::ObjectPicker`, the same ray walk the left-click touch uses),
and the **nearer** hit wins. The linkset resolution (picked prim → root,
combined `PrimFlags`, attachment detection) is `ObjectState::pick_summary` in
`objects.rs`, which now tracks each object's last-seen update flags. The
selection core remains its own task (multi-select, rubber-band, the
select/deselect protocol); its `blocked_by` edge here became a `refs` entry.

**Wired for real:** Touch (with the right-click ray's own `SurfaceInfo`, so
`llDetectedTouch*` reads the true hit; enabled on `FLAGS_HANDLE_TOUCH`); Sit
Here / Stand Up as the reference's **autohide chain** at one position
(`PieContent::Chain`, its first live use; sit targets the picked prim with
the object-local hit as offset); Take / Take Copy (both its addresses) /
Delete / Return as `DerezObjects` on the linkset root (Objects folder,
Objects folder copy, Trash, return-to-owner); Mute (an open-time
`RequestObjectPropertiesFamily` supplies the name). Everything else sits
greyed at its reference position via the `UNIMPLEMENTED` sentinel. The enable
gates are deliberately simplified flag reads — see
[[viewer-object-pie-enable-fidelity]] for the reference-faithful predicates
(and the mute empty-name race).

### Deliberate departures, each with its follow-up task

- **No Buy / Take autohide chain at west (yet).** The reference chains a Buy
  slice over the `Take >` sub-pie; buying is unwired and a chain member that
  is a *sub-pie* is not expressible in `pie_menu` today, so west holds
  `Take >` plainly and Buy keeps its other reference address (More >
  south-east) as a greyed slice → [[viewer-object-pie-buy-take-chain]].
- **Take's multi-selection slices are placeholders** (no multi-selection
  yet); the reference's two separator slots stay empty →
  [[viewer-object-pie-multi-select-take]].
- **`Attach HUD >` is declared empty** (renders disabled) — the reference
  fills it (and `Attach >`'s plain points) at runtime; the static Bento
  `Ext. Skeleton >` tree is reproduced in full as greyed slices, pinning the
  Bento addresses. Runtime lists land with wearing
  ([[viewer-inventory-attach-to-point]] and the re-lay in
  [[viewer-object-menu-reorder-when-implemented]]).
- **Worn attachments open nothing** — they belong to
  [[viewer-attachment-context-menu]] / [[viewer-hud-context-menu]].
- **The muted-particle-source pie is deferred**: its one slice needs particle
  picking, which the renderer does not do yet →
  [[viewer-particle-pick-mute]].
- **Flexi prims pick against a stale broad-phase `Aabb`** (a shared limit
  with left-click touch) → [[viewer-flexi-prim-picking]].
