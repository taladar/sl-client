---
id: viewer-notification-history
title: Notification list / history panel
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The notification **list / history** panel: a scrollback of the toasts the
notification host ([[viewer-ui-notification-host]]) has shown, so a dismissed or
missed dialog can be reviewed and, where still actionable, re-opened. This is
the "nearby / recent notifications" well the reference viewer keeps.

Builds on the notification host, which owns the live toasts; this task is the
persistent list over the same notification stream.

Reference (Firestorm, read-only): `llnotificationmanager`,
`llui/llnotifications`, `lltoast*`.

Builds on: [[viewer-ui-notification-host]].
