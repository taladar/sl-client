---
id: viewer-social-people-panel
title: People panel — friends / nearby / recent / blocked
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-social-panels
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-virtualized-list]
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
