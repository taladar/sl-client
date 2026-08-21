---
id: viewer-people-lists-multi-select
title: Multi-select in the People panel's lists (blocked, contact sets, groups)
topic: viewer
status: ready
origin: user question (2026-08-21) while building viewer-minimap-menu-multi-avatar
blocked_by: []
refs:
  [
    viewer-social-people-panel,
    viewer-block-list,
    viewer-contact-sets,
    viewer-conference-start-ui,
    viewer-radar-multi-select,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

Every list in the People panel is single-select today —
`SelectedFriend(Option<FriendKey>)`, `SelectedGroup(Option<GroupKey>)`,
`SelectedBlocked(Option<BlockedKey>)`, `SelectedMember(Option<AgentKey>)` —
so every action on them is one row at a time. The reference (and plain use)
wants several:

- **Blocked list** — unblock several entries at once. Deliberately left out of
  [[viewer-block-list]] ("the list is single-select, like Linden's"), but it is
  a Firestorm addition worth having: an accumulated mute list is exactly where
  one wants to clear ten rows.
- **Contact sets** — Move to Set… / Remove from Set / Add Resident for several
  members at once. The model half is already there: the add-to-set floater
  takes a list of residents and files them under one set
  ([[viewer-minimap-menu-multi-avatar]] made `OpenAddToContactSet` a list, with
  the reference's counted success notification), and Add Resident… already
  opens the shared picker in its multi mode
  ([[viewer-avatar-picker-multi-pick]]), so the panel only needs to be able to
  *pick* several of its own rows.
- **Groups sub-tab** — leave several groups at once (lower value; include only
  if it falls out of the same selection work).

The friends list itself is [[viewer-conference-start-ui]]'s: that task already
owns "multi-select in the people / friends lists" for the conference / invite
case, so this one covers the *other three* lists and should follow whatever
selection idiom it settles on.

The widget half exists — `ui_table.rs`'s `TableSelectionMode::Multi` with its
unit-tested Ctrl-toggle / Shift-range `apply_click` — and has **no consumer
yet**. The work here is adopting it (and keeping a selection keyed by the row's
identity, not its index, since these tables re-sort and re-filter under the
user).
