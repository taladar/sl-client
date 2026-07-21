---
id: viewer-social-groups
title: Groups list (Info / IM / Activate / Leave)
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-social-group-profile, viewer-social-people-panel]
---

Context: [context/viewer.md](../context/viewer.md).

The **groups** surface: the member's group **list**, hosted (like the Friends
list) in the **Groups** sub-tab of the People pane inside the Conversations
floater — the reference Vintage skin's arrangement (`panel_fs_contacts_groups`
with a `group_list` widget + a bottom action bar). The list is a virtualized
list ([[viewer-ui-virtualized-list]]).

The group protocol already exists (Groups V2); this task is the list and its
row actions.

Reference (Firestorm, read-only): `llgrouplist`, `llgroupactions`, Vintage
`panel_fs_contacts_groups`.

Builds on: `protocol-2` IM and the group model, and
[[viewer-social-people-panel]] (which owns the Groups sub-tab slot).

## Scope (2026-07-21): list + Info / IM / Activate / Leave only

On the user's steer the group **profile** (general / members / roles / notices,
and any role or membership editing) is split into its own task,
[[viewer-social-group-profile]], and reached later from this list's Info button.
This task builds only the **list** and the four per-group actions laid out like
the Friends list: **Info** (present in the layout but **inert** — it opens the
profile, which is the other task), **IM**, **Activate**, **Leave**.

## Done (2026-07-21)

New viewer module `groups.rs` + a `GroupsPlugin`, filling the **Groups**
sub-tab content slot the People pane ([[viewer-social-people-panel]]) already
carried as a placeholder — the same deferred-into-another-plugin arrangement the
People pane itself uses with the Conversations floater's strip.

- **Pure model + ECS mirror.** `GroupsModel` (unit-tested) is fed only from the
  `SlEvent` stream — `GroupMemberships` (the agent's full membership list,
  pushed on login and on change, so it replaces the cache wholesale),
  `ActiveGroupChanged` (the worn group), and `DroppedFromGroup` /
  `AgentDroppedFromGroup` / a successful `LeaveGroupResult` (drop lifecycle).
  Unlike the friends list it needs **no** name-resolution pass — each membership
  record already carries its group name (short-id placeholder only for the rare
  empty name). `GroupsView` is the ordered projection (case-folded by name) the
  virtualized list binds recycled rows to.
- **Table + selection + actions.** A persistent header (**Name** + **Active**)
  over a virtualized list; the active (worn) group's row is accented and shows a
  ● marker in the Active column, plus a group-count line under the list.
  Clicking a row selects it (highlighted) and focuses the list for the wheel; a
  **trailing** action column acts on the selection — **Info** / **IM** /
  **Activate** / **Leave**, matching the Friends list layout. **IM** opens (and
  joins) the group's chat tab — `conversations::OpenConversation` for the tab +
  `StartGroupSession` for the session. **Activate** sends
  `ActivateGroup(Some(..))`. **Leave** opens a confirm modal (mirroring the
  People pane's grant-confirm) that sends `LeaveGroup` and optimistically drops
  the group. **Info** is present but wired to nothing (the profile floater is
  [[viewer-social-group-profile]]).
- **Sharing the pane.** `people.rs` now exposes its Groups content slot
  (`PeopleUi::groups_content`) and spawns it as an empty container (the Groups
  placeholder text / fluent key is gone); this module owns what is *inside* it
  and never touches the sub-tab visibility the People pane already drives.

**Deferred / not in this task (differences from the original roadmap title):**
the entire group **profile** — general / members / roles / notices and every
role or membership mutation — is [[viewer-social-group-profile]]. The model, the
action→command mapping and the active-group marking are unit-tested; the live
list needs a group membership on the grid, so final on-screen verification is an
interactive run (open the People tab → Groups sub-tab, select a group, try
Activate / IM / Leave).
