---
id: viewer-notification-catalogue-diagnostics
title: Notification catalogue — diagnostics entries
topic: viewer
status: done
origin: notification-host coverage audit (2026-07-29) — full notifications.xml
  accounting; one follow-up per feature family
blocked_by: [viewer-ui-notification-host]
refs: [viewer-notification-catalogue]
---

Context: [context/viewer.md](../context/viewer.md).

Port the **diagnostics** notifications from the reference `notifications.xml`
into the declarative catalogue (`NOTIFICATIONS` in `src/notifications.rs`), the
way [[viewer-notification-catalogue]] ported the generic server-alert and
shared-confirm entries. These cover viewer-internal errors: asset / texture /
mesh load failures, GL, crash, unsupported hardware and out-of-memory.

The exact entry set is the **43** rows tagged `family:diagnostics` with
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

All 43 manifest rows ported (flipped to `ported` in the coverage TSV):
the installation / environment / hardware warnings (the `UnsupportedHardware`
/ `OldGPUDriver` visit-the-URL confirms and the ignorable `UnknownGPU` /
`NoHavok` notices), the file-handling failure modals (upload / resource /
generic file I/O), the low-memory pair, the `OutOfDiskSpace` unique tip,
the `RegionCapabilityRequestError` alert and the four persistent local
bitmaps / GLTF watcher notifies. Two new button forms: `SEND_CANCEL_FORM`
(stable `OK` / `Cancel` under a "Send" label) and `YES_NO_BUTTONS_FORM`
(the reference's explicit Yes / No functor names and labels, affirmative
default per the one-default invariant).

Deviations, noted in the Fluent block header and per entry: `[APP_NAME]` /
"SL" self-references reworded ("the viewer"); the Firestorm / Second Life
knowledge-base URLs trimmed from `BadInstallation` /
`FoundLegacyNsisInstallation` pending linkification; `CannotWriteFile`'s
literal-bracketed `[[FILE]]` reduced to the plain token (the `[KEY]` engine
would misparse it); Firestorm's "Avatar > Preferences" menu path shortened
to ours. The `<url>` open-on-affirmative actions belong to the raising
feature, not the data entry.
