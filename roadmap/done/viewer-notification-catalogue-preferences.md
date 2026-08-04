---
id: viewer-notification-catalogue-preferences
title: Notification catalogue — preferences & settings entries
topic: viewer
status: done
origin: notification-host coverage audit (2026-07-29) — full notifications.xml
  accounting; one follow-up per feature family
blocked_by: [viewer-ui-notification-host]
refs: [viewer-notification-catalogue]
---

Context: [context/viewer.md](../context/viewer.md).

Port the **preferences & settings** notifications from the reference
`notifications.xml` into the declarative catalogue (`NOTIFICATIONS` in
`src/notifications.rs`), the way [[viewer-notification-catalogue]] ported the
generic server-alert and shared-confirm entries. These cover the Preferences
panes (graphics, cache, hardware, privacy).

The exact entry set is the **110** rows tagged `family:preferences` with
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

Ported as part of the consolidated remaining-families port (108
rows now `ported` in the coverage TSV): entries generated mechanically
from the reference XML (kind / persist / unique / priority /
`log_to_chat` / ignore flag / forms / text inputs / titles), bodies with
the standard trims (bracketed URLs, `[APP_NAME]` / `[SECOND_LIFE]`
self-references, `<nolink>` markup), and the server-keyed subset pinned
in `keyed_server_alerts_are_catalogued`.

`ResetShowNextTimeDialogs` / `SkipShowNextTimeDialogs` are
commented out in the reference and re-tagged `excluded` in the TSV.
