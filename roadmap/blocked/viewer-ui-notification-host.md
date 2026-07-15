---
id: viewer-ui-notification-host
title: Notification / toast host
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework/viewer-notifications-dialogs
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The toast / notification **host**: the container surface that stacks, times out
and dismisses transient notifications, plus a notification list / history. This
is the shared substrate the specific dialogs sit in — the script permission
dialog ([[viewer-permission-request-dialog]]), the experience-acceptance prompt
([[viewer-experience-permission-dialog]]), and the remaining dialogs still
tracked by [[viewer-dialog-offers-invites]] (inventory / teleport /
friendship / group offers and notices).

Model the reference's **declarative notification catalogue** (notification types
declared as data, not code). Styling comes from the [[viewer-ui-skin-tokens]]
tokens.

Reference (Firestorm, read-only): `llui/llnotifications`,
`llnotificationmanager`, `lltoast*`, `llnotification*handler`.
