---
id: viewer-group-insignia-editing
title: Group profile — set the group insignia
topic: viewer
status: ready
origin: user request (2026-07-27), while implementing viewer-social-group-profile
refs: [viewer-social-group-profile, viewer-ui-texture-picker]
---

Context: [context/viewer.md](../context/viewer.md).

The group profile floater ([[viewer-social-group-profile]]) **displays** the
group insignia but cannot **set** it — its General-tab Save keeps the existing
`insignia_id` (the same display-only treatment the avatar profile gives its
pictures). The texture picker ([[viewer-ui-texture-picker]]) already exists, so
this is only the wiring:

- make the General-tab insignia box (for an agent holding
  `group_powers::GROUP_CHANGE_IDENTITY`) open the picker and carry the choice
  into `UpdateGroupInfoParams.insignia_id` on Save.

The `UpdateGroupInfo` path already carries the id — only the picker wiring is
missing. Mirrors [[viewer-profile-image-editing]].
