---
id: viewer-notifications-dialogs
title: Notifications, toasts & dialogs
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The interactive dialogs the grid throws at the user: `llDialog` script dialogs
and textbox prompts, inventory offers, teleport offers / lures, friendship and
group invites, and group notices — with a notification list / history.

The toast / notification **host** itself is now a concrete task
([[viewer-ui-notification-host]]), and the **script permission-request** scope
was carved out into concrete tasks — [[viewer-permission-request-dialog]],
[[viewer-permission-active-grants]] and [[viewer-experience-permission-dialog]]
— because the script-control tasks depend on them. This idea keeps the remaining
dialogs.

Much of the underlying protocol (teleport offers, permission requests) is
already handled; this stub is the specific remaining dialog panels and their
accept/decline wiring on top of the notification host.

Reference (Firestorm, read-only): `llui/llnotifications`,
`llnotificationmanager`, `lltoast*`, `llnotification*handler`,
`lltoastscriptquestion`, `llscriptfloater`.

Builds on: the teleport-offer and permission protocol already done.

Deps: [[viewer-ui-notification-host]].
