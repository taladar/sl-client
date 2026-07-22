---
id: viewer-social-group-extras
title: Group profile extras — land/money, experiences, bans, create/invite
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-social-group-profile]
---

Context: [context/viewer.md](../context/viewer.md).

The group-profile pieces [[viewer-social-group-profile]] explicitly leaves
out:

- **Land / Assets tab** — group-owned parcels + contribution editing
  (`GroupLandUpdate` family; `api-g10` money/land data).
- **Money tab** — account summary / details / transactions and planning
  (`api-g10` `GroupAccount*` — protocol + conformance test done).
- **Experiences tab** — the group's experiences (`protocol-27` data).
- **Banned residents** — the group ban list caps (list / ban / unban,
  incl. ban duration; verify the `GroupAPIv1` ban cap pairing exists in
  `sl-proto`, add if missing) and the bulk-ban panel.
- **Create group** — the creation dialog (`CreateGroupRequest`, fee
  confirmation; `test-group-create-activate` proves the wire).
- **Invite to group** — the invite dialog (resident multi-pick + role
  choice, `InviteGroupRequest`), reached from the profile and the avatar
  context menu.

Reference (Firestorm, read-only): `llpanelgrouplandmoney`,
`panel_group_land_money.xml`, `llpanelgroupexperiences`,
`llpanelgroupbulkban`, `panel_group_invite.xml`,
`panel_group_creation_sidetray.xml`.

Deps: [[viewer-social-group-profile]] (the tabbed floater these extend).
