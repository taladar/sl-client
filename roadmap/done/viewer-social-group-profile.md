---
id: viewer-social-group-profile
title: Group profile floater — general / members / roles / notices
topic: viewer
status: done
origin: split from viewer-social-groups (2026-07-21) — the list shipped, the
  profile is its own task
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
refs: [viewer-social-groups]
---

Context: [context/viewer.md](../context/viewer.md).

The group **profile**: a separate floater (the Vintage skin opens
`FSFloaterGroup` / `panel_group_info_sidetray` rather than embedding it in the
Contacts floater), reached from the [[viewer-social-groups]] list's **Info**
button — which is present-but-inert until this task wires it. Tabs:

- **General** — name, insignia, charter, founder, member / role counts, join
  fee, open enrollment, mature flag; the agent's own membership toggles (receive
  notices / list in profile → `SetGroupAcceptNotices`), active-title combo
  (`UpdateGroupTitle`), and Join.
- **Members & Roles** — the members list (virtualized: name / title /
  contribution / status) and the roles list, the abilities (`group_powers`
  `GP_*`) viewer, and the **mutations**: eject member (`EjectGroupMembers`),
  member↔role assignment (`ChangeGroupRoleMembers`), and role create / delete /
  ability edits (`UpdateGroupRoles`) — all power-gated on the agent's own
  `group_powers`.
- **Notices** — the notice list, viewing a notice's full body
  (`RequestGroupNotice`), and composing / sending one (`SendGroupNotice`,
  gated on `NOTICES_SEND`).

The whole group protocol already exists (Groups V2) and all the commands /
events above are re-exported through `sl-client-bevy`; this task is the panels
that present and mutate the profile. Note `UpdateGroupInfoParams` and
`GroupName` are **not yet** re-exported from `sl-client-bevy` (only
pattern-matchable via the event) — export them if General-tab editing needs to
name the type.

Not in scope here (as in the reference's other tabs): Land / Assets, Money /
accounting, Experiences, Banned Residents, and the group create / search /
invite dialogs — all filed as [[viewer-social-group-extras]] (search lives in
the search floater).

Reference (Firestorm, read-only): `llpanelgroup`, `llpanelgroupgeneral`,
`llpanelgrouproles`, `llpanelgroupnotices`, `llgroupmgr`, Vintage
`floater_fs_group` / `panel_group_info_sidetray`.

Builds on: [[viewer-social-groups]] (the list + its inert Info button) and the
group model.

Note (2026-07-22): this floater is **subject-bound** — it opens on a
particular subject rather than persistent app state — so exempt it from
floater persistence (`floater_persist::FloaterPersistExempt` on the root,
as the avatar profile and item previews do): no restored rectangle, no
restored "open".

## Done (2026-07-27)

New module `group_profile.rs` + `GroupProfilePlugin`, a subject-bound
`FloaterPersistExempt` floater with three tabs, reached from the
[[viewer-social-groups]] list's now-live Info button **and** by double-clicking
a group in the [[viewer-avatar-profile-group-list]]. `UpdateGroupInfoParams` was
re-exported from `sl-client-bevy` (+ `sl-client-tokio` for parity) so
General-tab editing can name the type.

- **General** — insignia, name, founder, member/role counts, join fee, charter,
  open-enrollment / mature / show-in-list (editable + `UpdateGroupInfo`, gated
  on `GROUP_CHANGE_IDENTITY`/`MEMBER_OPTIONS`); membership toggles
  (receive-notices / list-in-profile → `SetGroupAcceptNotices`), active-title
  cycle (`UpdateGroupTitle`), Join.
- **Members & Roles** — virtualized members list (name/title/land/status,
  accumulated + deduped across replies with a "loaded N of TOTAL" line +
  Refresh, since SL's first fetch is just officers/owners), roles list,
  `group_powers` abilities viewer, and power-gated mutations:
  `EjectGroupMembers`, `ChangeGroupRoleMembers`, `UpdateGroupRoles`
  (create/delete/rename/abilities).
- **Notices** — virtualized list, body view (`RequestGroupNotice` → the
  `GroupNotice` IM), compose/send (`SendGroupNotice`, gated on `NOTICES_SEND`).

**Verified** live (OpenSim, member of real groups; earlier passes on aditi).

**Cross-cutting fallout (bigger than the floater itself):** opening it exposed a
latent **UI churn** crash — the avatar/group profiles rebuilt panels
(`despawn_children` + respawn) on every streaming reply, and a node despawned
within a frame of being built races `bevy_flair`'s deferred `ClassList` insert
(it styles every `Node`). Both floaters were converted to **retained-mode**
(build structure once per subject, update values in place;
virtualized/reconciled lists) so nothing is spawned-and-despawned in the same
frame. `try_insert` band-aids were tried then reverted (see the "never hide
errors" memory).

**Deferred (see the follow-ups):** notice **attachments**
([[viewer-group-notice-attachments]]), insignia **editing**
([[viewer-group-insignia-editing]]), the reply-driven churn refactor of the
*other* detail floaters ([[viewer-floater-update-in-place]]), locale-aware table
**ellipsis** ([[viewer-table-cell-ellipsis]]), one double-click-interval
constant ([[viewer-consolidate-double-click-interval]]), and some minor
group-profile layout polish. `list_in_profile` isn't on the wire membership, so
it defaults on.
