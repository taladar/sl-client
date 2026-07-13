---
id: viewer-notifications-dialogs
title: Notifications, toasts & dialogs
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The toast / alert system plus the interactive dialogs the grid throws at the
user: script permission requests, `llDialog` script dialogs and textbox
prompts, inventory offers, teleport offers / lures, friendship and group
invites, and group notices — with a notification list / history.

Much of the underlying protocol (teleport offers, permission requests) is
already handled; this stub is the notification host + the specific dialog
panels and their accept/decline wiring.

Reference (Firestorm, read-only): `llui/llnotifications`,
`llnotificationmanager`, `lltoast*`, `llnotification*handler`,
`lltoastscriptquestion`, `llscriptfloater`.

Builds on: the teleport-offer and permission protocol already done.

Deps: [[viewer-ui-framework]].
