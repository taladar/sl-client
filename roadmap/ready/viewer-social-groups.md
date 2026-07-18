---
id: viewer-social-groups
title: Groups list + group profile / roles / notices
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
---

Context: [context/viewer.md](../context/viewer.md).

The **groups** surface: the member's group **list**, and the group **profile**
with its members / **roles** and **notices** tabs. The list and the member /
role tables are virtualized lists ([[viewer-ui-virtualized-list]]) hosted in a
floater ([[viewer-ui-widget-scaffold]]).

The group protocol already exists (Groups V2); this task is the panels that
present and mutate group membership, roles, and notices.

Reference (Firestorm, read-only): `llpanelgroup*`, `llgroupmgr`.

Builds on: `protocol-2` IM and the group model.
