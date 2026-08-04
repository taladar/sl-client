---
id: viewer-notification-catalogue-estate-region
title: Notification catalogue — estate & region management entries
topic: viewer
status: done
origin: notification-host coverage audit (2026-07-29) — full notifications.xml
  accounting; one follow-up per feature family
blocked_by: [viewer-ui-notification-host]
refs: [viewer-notification-catalogue]
---

Context: [context/viewer.md](../context/viewer.md).

Port the **estate & region management** notifications from the reference
`notifications.xml` into the declarative catalogue (`NOTIFICATIONS` in
`src/notifications.rs`), the way [[viewer-notification-catalogue]] ported the
generic server-alert and shared-confirm entries. These cover estate-manager
tools, region debug / restart / telehub and kick / ban.

The exact entry set is the **111** rows tagged `family:estate-region` with
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

All 111 manifest rows ported (flipped to `ported` in the coverage TSV):
the region tools (top-object return / disable, terrain raw upload /
download / bake, map-cache flush, restart, announcements), the eight
unique terrain texture / material validation modals, the estate
access-list results, the admin kick / freeze / message prompts (six more
toast text-input dialogs — `KickUser`, `KickAllUsers`, `FreezeUser`,
`UnFreezeUser`, `MessageEstate`, `MessageRegion`), the 16 estate-scope
choosers on a shared This Estate / All Estates / Cancel form, the
pathfinding state notices, and the server-keyed entry refusals / freeze /
eject / terrain feedback (pinned in `keyed_server_alerts_are_catalogued`).

Because `MessageEstate` / `MessageRegion` start with an **empty** input,
`NotificationInput::default_key` became `Option<&'static str>` (a `None`
default resolves to an empty field). Six new button forms:
`THIS_ESTATE_ALL_ESTATES_FORM`, `KICK_ALL_RESIDENTS_CANCEL_FORM`,
`OK_CANCEL_DONT_ASK_FORM`, `BAKE_CANCEL_FORM`, `REBAKE_CLOSE_FORM`,
`REBAKE_REGION_FORM` — all on stable reference functor names.

Deviations, commented per entry: the `<nolink>` markup stripped from
`GroupIsAlreadyInList` (no linkification yet); the reference's
"All Estatees" label typo corrected by the shared All Estates label;
`ConfirmTextureHeights`' "Ok" label uses the shared OK label. The
reference `unique combine="cancel_old"` on the terrain validation family
maps to our unique-replace dedup (same net effect).

Dialog **titles** (the reference `label=`) are now first-class (decided
mid-task): `NotificationTemplate::title_key` is an optional Fluent key the
host renders as a header line above the body (`sk-toast-title` class,
swept by the gallery specimen). All 32 labeled entries ported so far —
across every family — carry their reference title through 16 shared
`notification-title-*` keys; a tip never carries one (pinned in
`kind_invariants_hold`). The history panel and preferences alerts tab
inherit the human-readable name from here.
