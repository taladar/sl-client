---
id: viewer-notification-catalogue
title: Notification catalogue — port the reference server-alert & confirm entries
topic: viewer
status: ready
origin: notification-host coverage audit (2026-07-29) — the catalogue is a
  5-entry seed; the bulk of reference notifications have no entry yet
blocked_by: [viewer-ui-notification-host]
refs:
  [
    viewer-generated-chat-notices,
    viewer-money-economy-ui,
    viewer-preferences-alerts-tab,
    viewer-teleport-flow-progress,
  ]
---

Context: [context/viewer.md](../context/viewer.md).

The notification host ([[viewer-ui-notification-host]]) ships a **5-entry seed**
catalogue (`NOTIFICATIONS` in `src/notifications.rs`) — enough to exercise every
kind, the `[KEY]` substitution and the dedup, but not the reference viewer's
actual notification set (~1,300 `notifications.xml` entries). This task grows
the catalogue to cover the notifications that have
**no bespoke dialog of their own** and today fall back to the generic raw-string
`SystemMessage` / `GenericAlert`:

- **Keyed server alerts** — the `AlertMessage` / `AgentAlertMessage` /
  `AlertInfo` messages the simulator sends by key (e.g.
  `RegionEntryAccessBlocked`, `CantTeleportToStore`, `NoRoomToSitHere`,
  `TooManyScripts`, `YouHaveBeenLoggedOut`, `MustAgreeToLogIn`, land / object /
  estate refusals). The reference maps each key to a localized entry with the
  right kind, buttons and `ignore` flag; we surface the raw string. Port the
  common keys as catalogue entries so `ingest_alert_messages` raises a proper,
  localized notification.
- **Standard action-confirmation modals** — the `alertmodal` confirms shared
  across features: return / delete objects, delete inventory, empty trash, leave
  group, remove friend, log out, "this will cost L$X" upload / buy confirms.
  Each is raised by its owning feature (object / inventory / groups / people /
  money menus), but the **entries** (text, buttons, ignore checkbox) belong in
  the shared catalogue.
- **Info tips / notifies** not already owned — landmark created, item received,
  snapshot saved, teleport-home, etc. (the ones not routed to nearby chat by
  [[viewer-generated-chat-notices]]).

Out of scope (each is its own bespoke notification task, not a catalogue entry):
the script dialogs / permissions ([[viewer-dialog-lldialog]],
[[viewer-permission-request-dialog]], [[viewer-experience-permission-dialog]],
[[viewer-dialog-script-load-url]]), inventory / teleport offers + friendship /
group invites ([[viewer-dialog-offers-invites]]), group notices
([[viewer-group-notice-display]]), the pay / buy flows
([[viewer-money-economy-ui]]), and the grid-status / region-tracker feeds. Those
own their forms and actions; this task only fills the **data catalogue** the
generic emitters raise from, and pairs with the alerts tab
([[viewer-preferences-alerts-tab]]) that manages the `ignore` flags.

Reference (Firestorm, read-only): `skins/default/xui/en/notifications.xml`,
`llnotifications`, the `LLNotificationsUtil::add` call sites in
`llviewermessage.cpp`, and the reference `notification_visibility.xml`.
