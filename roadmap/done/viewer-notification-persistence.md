---
id: viewer-notification-persistence
title: Persistent notifications — survive relog until answered
topic: viewer
status: done
origin: user request (2026-07-29), while implementing viewer-group-notice-display
  — an unclosed notice must re-display on next login, not be lost
blocked_by: [viewer-ui-notification-host, viewer-group-notice-display]
refs: [viewer-notification-history]
---

Context: [context/viewer.md](../context/viewer.md).

The reference `LLPersistentNotificationStorage` writes every `persist="true"`
notification still open (unanswered) to a per-account file
(`open_notifications_<grid>.xml`), reloads them at startup, and removes each one
when the user finally responds — so an unacknowledged alert / group notice
survives a relog. "Seen" is a **client-side** fact (the user closed it), not a
server acknowledgement: a plain group notice sends nothing on close (verified
against `LLToastGroupNotifyPanel::onClickOk`). This task adds that store.

## Done (2026-07-29)

- **Store** — new `notification_persist.rs` + `NotificationPersistPlugin`: a
  `PersistentNotificationStore` that serializes the open notifications to a
  per-account `open_notifications.json` (a sibling of the account
  `settings.toml`, via the account dir the `sl-account-dirs` per-avatar scope
  resolves at login). serde + serde_json added to the viewer crate.
- **Record / forget** — a producer writes a `PersistNotification { id, kind }`;
  the store forgets an entry when its `NotificationResponse` arrives (any
  answer, incl. a close ×). `NotificationId` is session-local (not serialized);
  the file is the ordered list of payloads.
- **Reload** — at login, once the per-account dir resolves, the store reads the
  file and **re-raises** each entry: a `Catalogue` entry via `ShowNotification`
  (template resolved back to its `&'static` name), a `Custom` entry via a
  `ReloadPersistedNotification` its owning module rebuilds from.
- **Producers** — the notification host persists every sticky (non-fading)
  `persist` catalogue toast it raises; `group_notice` persists each card as a
  `Custom` payload (a flat string map — group / sender / subject / body /
  timestamp / attachment; the insignia is re-derived from `GroupsModel` on
  reload, matching the reference) and reloads them through
  `reload_group_notices`.
- Unit-tested: the record/forget lifecycle, the JSON payload round trip, and the
  group-notice encode↔decode round trip (incl. optional fields + a missing group
  id → `None`).

Fading tips / notifies are **not** persisted (transient by nature); only
notifications that require an acknowledgement survive a relog.

**Verification:** the store logic is unit-tested; the end-to-end relog behavior
(receive a notice → quit without answering → relog → it reappears → answer →
relog → it is gone) needs a live grid, so it is checked by running the viewer.
