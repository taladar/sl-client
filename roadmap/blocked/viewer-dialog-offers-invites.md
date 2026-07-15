---
id: viewer-dialog-offers-invites
title: Inventory / teleport offers + friendship / group invites
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-notifications-dialogs
blocked_by: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The accept / decline dialogs the grid throws at the user: **inventory offers**,
**teleport offers / lures**, **friendship** and **group invites**, and **group
notices** (with their attachments). Each is a toast on the notification host
([[viewer-ui-notification-host]]) with accept / decline (and discard / mute)
buttons wired to the existing protocol replies.

Much of the underlying protocol — teleport offers, inventory offers, friendship
and group invites — is already handled; this task is the remaining dialog panels
and their accept / decline wiring on top of the notification host.

Reference (Firestorm, read-only): `llnotificationmanager`,
`lltoast*`, `llnotification*handler`.

Builds on: the teleport-offer, inventory-offer and invite protocol already done.
