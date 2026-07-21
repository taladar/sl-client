---
id: viewer-social-people-panel
title: People panel — friends / nearby / recent / blocked
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-social-groups, viewer-avatar-radar, viewer-social-profiles]
---

Context: [context/viewer.md](../context/viewer.md).

The **people** panel: the tabbed avatar-list surface — **friends**, **nearby**,
**recent**, and **blocked** — plus the per-avatar actions that hang off each row
(profile, IM, offer teleport, add friend, block / unblock, and so on). Each tab
is a virtualized avatar list ([[viewer-ui-virtualized-list]]) hosted in a
floater ([[viewer-ui-widget-scaffold]]); rows show name + presence and open the
context actions.

The friend / presence / block protocol already exists; this task is the
interactive panel over it — list rendering, tab switching, and wiring the row
actions to the existing commands.

Reference (Firestorm, read-only): `llpanelpeople`, `llavatarlist`.

Builds on: `protocol-2` IM and the friend / presence model.

## Scope (2026-07-21): Vintage-style Contacts integration, Friends only

The reference **default** skin keeps People (`floater_people`) and Conversations
(`llfloaterimcontainer`) as two windows, but the **Vintage** skin folds them
into one: its Conversations floater is a `multi_floater` with a left tab strip,
and the "Contacts" floater (`floater_fs_contacts`) is *hosted* as one left tab
whose content is a horizontal `tab_container`. On the user's steer this task
reproduces that arrangement rather than a standalone floater, and narrows the
tab set:

- **nearby** (avatars with distances) is the separate **radar**
  ([[viewer-avatar-radar]]), not this task.
- **recent** and **blocked** are **not built** this pass (deferred).
- **groups** has its own task ([[viewer-social-groups]]); a placeholder sub-tab
  is present so the container matches Vintage.

So the delivered surface is a single pinned **People** tab in the Conversations
floater's vertical strip, whose pane hosts a horizontal **Friends / Groups**
sub-tab strip: Friends is the live list, Groups is a stub.

## Done (2026-07-21)

New viewer module `people.rs` + a `PeoplePlugin`, hung off the Conversations
floater as its own pinned, un-closable strip tab (beside Nearby Chat), reusing
that floater's strip and panel area.

- **Pure model + ECS mirror.** `FriendsModel` (unit-tested) is fed only from the
  `SlEvent` stream — `FriendList` / `FriendsSnapshot` (buddy cache),
  `FriendsOnline` / `FriendsOffline` (presence), `FriendRightsChanged`,
  `FriendshipTerminated`, and `AvatarNames` (name resolution). `FriendsView` is
  the ordered projection (online first, then case-folded name) the virtualized
  list ([[viewer-ui-virtualized-list]]) binds recycled rows to; unresolved names
  request `RequestAvatarNames` once each and show a short-id placeholder until
  they land. First open sends one `QueryFriends` to seed; the granular events
  keep it live after.
- **Table + selection + actions.** A persistent header (shown even when empty)
  over a virtualized table: **Name**, **Status** (presence dot ● / ○), and — per
  the Vintage `panel_fs_contacts_friends` columns — the friendship rights in
  **both directions**, grouped **They** (rights this agent grants: see-online /
  map / edit-my-objects) and **You** (rights the friend grants: see-online / map
  / edit-theirs). Clicking a row selects it (highlighted) and focuses the list
  for the wheel; a **trailing** action column acts on the selection — **IM**,
  **Offer Teleport**, **Remove Friend**, **Block** — mapping to `OfferTeleport`
  / `TerminateFriendship` / `Mute{MuteType::Agent}`; **IM** instead opens a
  one-to-one conversation tab in the same floater via the new
  `conversations::OpenConversation` hook (no dead-end).
- **Generated icons + editable checkboxes.** The rights columns render as
  **checkboxes** with **procedurally-drawn icons** (`build_icon` rasterises an
  eye / map-pin / pencil column header and ticked / empty checkboxes into RGBA
  `Image`s at startup, tinted via `ImageNode::color` — no binary art in the
  crate). The **They** (granted) checkboxes are **interactive**: a click flips
  the bit and sends `GrantUserRights` (optimistic local flip; the server echo
  re-confirms). The **You** (received) checkboxes are read-only, dimmed (the
  friend controls those). Row cells are recycled `ImageNode`s carrying a
  `CellFriend` updated on bind so a toggle acts on the right friend.
- **Sortable, persistent, multi-column.** Every header sorts on click: the
  clicked column becomes primary (a second click flips its direction) and the
  previous order demotes to tie-breakers — "sort by the last-clicked column,
  then the one before, …" (`SortState`, capped at 6 levels). Default is
  **online-first, then name** (so online friends alphabetical, then offline
  alphabetical). The primary column shows a ▲ / ▼ arrow (Name / Status). The
  order **persists per avatar** via a `[people] friends_sort` account setting
  (`register` / seed-after-`load_account_settings` / save-on-change, reusing
  `crate::settings`).
- **Tab placement.** The People tab is inserted **first** in the strip (above
  Nearby Chat, via `insert_child(0, …)`) so every chat tab stays grouped below
  it.
- **Shared strip arbitration.** A new generic `conversations::StripFocus`
  resource decides whether a **conversation** pane or an **external** pane (this
  People tab) owns the shared panel area, so exactly one shows; selecting a
  conversation hands the strip back, selecting People takes it.
  `ConversationsUi` gained `strip()` / `panel_area()` accessors; the People
  tab/pane are spawned into them by a deferred once-system (robust across the
  two plugins' resource insertion).
- **Groups placeholder.** The horizontal sub-tab strip carries Friends + Groups;
  Groups shows an explanatory line, its real list left to
  [[viewer-social-groups]].
- **Parity.** `FriendKey` / `FriendPresence` are now `pub`-exported from
  `sl-client-bevy` and `sl-client-tokio` (they were internal to the
  event/command enums before).

**Deferred / not in this task (differences from the roadmap title):** the
**Recent** and **Blocked** tabs; the **nearby**/radar list
([[viewer-avatar-radar]]); the **Profile** row action and its floater
([[viewer-social-profiles]]); an **add-friend** avatar picker; a confirmation
prompt before granting **edit-my-objects** (the reference warns; we toggle
directly). The friends model, the action mapping, the rights-column mapping, the
rights toggle and the sort (click / encode / parse) are unit-tested; the live
Friends list needs a friend on the grid, so final on-screen verification is an
interactive run (open the People tab, select a friend, toggle a right, click the
headers to sort).
