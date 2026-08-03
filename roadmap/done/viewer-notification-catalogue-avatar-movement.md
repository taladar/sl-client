---
id: viewer-notification-catalogue-avatar-movement
title: Notification catalogue — avatar movement entries
topic: viewer
status: done
origin: notification-host coverage audit (2026-07-29) — full notifications.xml
  accounting; one follow-up per feature family
blocked_by: [viewer-ui-notification-host]
refs: [viewer-notification-catalogue]
---

Context: [context/viewer.md](../context/viewer.md).

Port the **avatar movement** notifications from the reference
`notifications.xml` into the declarative catalogue (`NOTIFICATIONS` in
`src/notifications.rs`), the way [[viewer-notification-catalogue]] ported the
generic server-alert and shared-confirm entries. These cover sit / stand,
autopilot, fly, physics and animation upload.

The exact entry set is the **40** rows tagged `family:avatar-movement` with
`status=followup` in the coverage manifest — [context/notif-
coverage.tsv](../context/notif-coverage.tsv), the complete accounting of every
reference notification. Port each with its reference kind (`notify` /
`notifytip` / `alert` / `alertmodal`), buttons, `priority`, `persist`,
`log_to_chat` and `ignore` flag, plus its Fluent body in `en/main.ftl`. Trim any
bracketed knowledge-base URLs from the body until the linkification layer lands
(the `[KEY]` engine reads `[...]` as a substitution token).

This is the **data** entry only; the owning feature raises each notification at
its call-site and reads the response. Entries whose reference form carries a
feature callback (the maturity `_Change` / `_AdultsOnlyContent` variants,
dynamic button lists) wait on that feature's plumbing.

## Done

39 of the 40 manifest rows ported (flipped to `ported` in the coverage
TSV): the animation-upload failures and AO set-management prompts
(`NewAOSet` reuses the toast text-input field the appearance-wearables
task added), the scripted-control notice, the server-keyed sit / stand
refusals (pinned in the `keyed_server_alerts_are_catalogued` test), the
15-entry AO notecard-import progress tips and the phantom / movelock /
flight-assist toggle tips. One new button form, `REMOVE_CANCEL_FORM`
(stable `OK` / `Cancel` names under a "Remove" label).

The 40th row, `notifyignore`, turned out not to be a notification at all —
it is a `<template>` element (an ignore-only form template) the coverage
audit swept up; its TSV row is re-tagged with a new `excluded` status,
documented in the TSV header.

Deviations, commented at the entry: `[APP_NAME]` reworded ("The viewer")
in `DoNotSupportBulkAnimationUpload`; `ConfirmPoserOverwrite`'s "Okay"
label uses the shared OK button label.
