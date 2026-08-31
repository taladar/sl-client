---
id: viewer-world-pie-menu-reactions
title: Right-click reactions per world target class
topic: viewer
status: done
origin: user request (2026-07) — right-click avatar must show the pie menu
points: 5
blocked_by: [viewer-world-test-harness]
---

Context: [context/testing.md](../context/testing.md).

Done (2026-08-31). The three halves of the task landed in three places:

- **which pie a right-click opens** — all six target classes, through the
  real classifier and the CPU pick resolver, in
  [[viewer-world-pie-target-tests]];
- **what each pie holds** — the four committed compass-address tables in
  `object_menu.rs`, `avatar_menu.rs`, `attachment_menu.rs` and
  `land_menu.rs`, beside the per-menu condition tests that pin which
  slices are live in which state;
- **what a committed slice does** — this work: `world_test.rs`'s
  `pie_dispatch_tests`, sixteen tests over `handle_object_menu_actions`,
  `handle_avatar_menu_actions`, `handle_attachment_menu_actions` and
  `handle_land_menu_actions`.

Every dispatch test opens its pie with a **real right-click** in the
fixture world, so the target the slice acts on is the one the classifier
resolved, and then writes the `UiAction` the widget emits when the slice
is clicked. Re-clicking a label per action would re-test the ring rather
than the dispatch: the label→action half is pinned by the address tables
and, end to end, by `a_pie_slice_clicked_in_world_sends_its_command`.

What the sixteen pin:

- **object**: Touch names the *picked* prim (and carries the ray's
  surface) while Open asks for the *linkset root's* contents — the
  root/part distinction, driven by right-clicking a linkset child; Edit
  selects the root, and with Edit Linked Parts on, the part; Sit Here
  names the picked prim as the seat and Stand Up clears the tracked
  ground sit; each derez lands in the folder its slice names (Objects for
  Take and Take Copy, Trash for Delete) and is *dropped* — not derezzed
  into nowhere — while that folder is unknown, with Return, which needs
  none, going out either way; Block raises a `RequestBlock` whose guard,
  not the pie, is what puts the `Mute` on the wire, and both Derender
  slices write only the local request, differing in `permanent`.
- **avatar**: IM, Profile and Refresh Textures open on the clicked agent;
  Add as Friend offers on the wire; Block records the name the *grid*
  resolved (fed as a real `AvatarNames` reply, not a poked cache); the
  self pie's Sit Down / Stand Up drive the tracked ground sit the wire
  never reports back; and the avatar-only slices — Derender, the render
  overrides, Add to Set, Set Alias — are dispatched for the avatar
  element alone, with the same six action names under the attachment
  element reaching nothing.
- **attachment**: Detach, Drop and Touch act on the worn object; a
  Derender there hides the *attachment*, not its wearer (both handlers
  see that action name and exactly one must answer); and the
  wearer-derived slices — IM, Add as Friend — reach the wearer the open
  stashed, which is the only reason right-clicking someone's hat can
  start a conversation with them.
- **land**: Sit Here stands an already-seated avatar up *first* (the
  order matters — an object-seated avatar ignores the ground-sit control
  bit), with the unseated run as the control; About Land opens on the
  ground point that was clicked, not the agent's own parcel.

Two notes for whoever reads this next:

- the task's "reach/distance and permission gating where the handlers
  apply it" is satisfied vacuously: the handlers apply none. Every such
  gate lives one level up, in the pie's `PieConditions` (a non-touchable
  object greys Touch, an un-copyable one greys Take Copy), where the
  per-menu condition tests already pin it. **Pay** likewise has no
  mapping to pin — it is still a disabled placeholder.
- Edit's other half — that the Build Tools floater *opens* — is not
  visible in this tier: the UI fold carries no floater plugins, so
  `edit_picked_object` finds no panel to show and only the selection is
  observable. That assertion belongs to
  [[viewer-build-floater-interaction-tests]].

Two small enabling changes rode along: `RequestDerender` derives
`PartialEq`/`Eq` (its fields are crate-private to `sl-viewer-world-avatar`,
so a downstream test can only compare whole requests), and
`InventoryModel::merge_folders` is public (the model outlives its floater
— a fixture world holds the resource without the inventory window's
plugin, which is what folds the skeleton events in).
