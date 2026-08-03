---
id: viewer-notification-catalogue-appearance-wearables
title: Notification catalogue — appearance & wearables entries
topic: viewer
status: done
origin: notification-host coverage audit (2026-07-29) — full notifications.xml
  accounting; one follow-up per feature family
blocked_by: [viewer-ui-notification-host]
refs: [viewer-notification-catalogue]
---

Context: [context/viewer.md](../context/viewer.md).

Port the **appearance & wearables** notifications from the reference
`notifications.xml` into the declarative catalogue (`NOTIFICATIONS` in
`src/notifications.rs`), the way [[viewer-notification-catalogue]] ported the
generic server-alert and shared-confirm entries. These cover wearables, outfits,
the current-outfit folder, attachments and bakes.

The exact entry set is the **68** rows tagged `family:appearance-wearables` with
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

All 68 manifest rows ported (flipped to `ported` in the coverage TSV):
the outfit / wearable editing confirms and failures, the attachment
prompts, the appearance tips / notifies (including the 12-entry avatar-rez
diagnostics family) and the server-keyed attach / drop refusals, which
`ingest_alert_messages` now resolves by `AlertInfo` key (pinned in the
`keyed_server_alerts_are_catalogued` test). Six new button forms keep the
reference functor names stable under localized labels
(`YES_NO_FORM`, `SAVE_DISCARD_CANCEL_FORM`, `SAVE_ALL_DISCARD_CANCEL_FORM`,
`DISCARD_KEEP_EDITING_FORM`, `SAVE_CANCEL_FORM`, `REPLACE_ATTACHMENT_FORM`).

Because `SaveOutfitAs` / `SaveWearableAs` / `RenameOutfit` are text-input
prompts in the reference, the host gained a **single-line text-input form
field** (decided over porting them button-only): `NotificationTemplate::input`
(`NotificationInput { name, default_key }`, the pre-fill resolved through
Fluent then `[KEY]`-substituted with the raise args), rendered between body
and buttons via the shared `spawn_text_input` widget, and returned on
`NotificationResponse::input` when a button is chosen. Reusable by later
families (landmark / picks rename prompts).

Faithfulness deviations, each commented at the entry: `[APP_NAME]` reworded
("the viewer") in `ClothingLoading` / `InvalidWearable`; the Firestorm
debug-settings sentence trimmed from `TooManyWearables`;
`ReplaceAttachment`'s `save_option` remembered-response approximated by the
plain suppress flag (the `ConfirmQuit` precedent).
