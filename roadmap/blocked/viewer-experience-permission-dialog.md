---
id: viewer-experience-permission-dialog
title: Experience permission flow (accept / manage)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The **Experience** permission flow: the one-time experience-acceptance prompt
(accept / block) and a manage-experiences surface (allowed / blocked / forget).
This matters for permissions because a script running **under an accepted
experience** is auto-granted permissions without the per-request
[[viewer-permission-request-dialog]] — so accepting/blocking/forgetting an
experience is the real control point for that whole class of auto-grants.

Builds on the existing experience protocol (`experiences.rs`; the deferred
`protocol-34` experience key-value store is **not** required here).

Reference (Firestorm, read-only): `llfloaterexperiences`, `llexperiencelog`,
`llpanelexperiences`, and the `AgentExperience` / `ExperiencePermission` caps.
