---
id: viewer-notification-catalogue
title: Notification catalogue — port the reference server-alert & confirm entries
topic: viewer
status: done
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

## Done

Grew the catalogue (`NOTIFICATIONS` in `src/notifications.rs`) from its 5-entry
seed with the **generic, cross-feature** entries — the ones a feature does not
own — and set up the **complete accounting** the end goal wants: every one of
the reference's 1,329 `notifications.xml` entries now maps to exactly one home.

- **Ported here (20 entries)** — the keyed server alerts
  `ingest_alert_messages` raises when the simulator's `AlertInfo` key matches
  (the maturity / access family `RegionEntryAccessBlocked` /
  `TeleportEntryAccessBlocked` / `LandClaimAccessBlocked` /
  `LandBuyAccessBlocked` + `_Notify`, the region-restart `Seconds` companion,
  `TooManyScripts`, `FailedToPlaceObject`, `FailedToFindWearableUnnamed`,
  `HomePositionSet`), the standard shared confirms (`ConfirmEmptyTrash`,
  `RemoveFromFriends`, `GroupLeaveConfirmMember`, `YouHaveBeenLoggedOut`,
  `MustAgreeToLogIn`) and generic tips (`LandmarkCreated`,
  `GrantedModifyRights`, `TeleportToPerson`). Two custom button forms
  (`LEAVE_CANCEL_FORM`, `VIEW_IM_QUIT_FORM`) keep stable `OK` / `Cancel` names
  under localized labels; Fluent bodies in `en/main.ftl` are trimmed of the
  reference's bracketed KB URLs (the `[KEY]` engine reads `[...]` as a token —
  linkification is deferred). A test makes every catalogue `message_key` /
  `label_key` load-bearing against `en/main.ftl`.
- **Complete coverage accounting** — every reference notification is classified
  by owning feature family in
  [context/notif-coverage.tsv](../context/notif-coverage.tsv): `ported` (the 20
  above), `done` (16 already handled by a bespoke dialog task — script dialogs /
  permissions / offers / group notice / experience prompt), or `followup` (1,293
  across 27 feature families).
- **Per-family follow-ups** — one `viewer-notification-catalogue-<family>` task
  per family (objects-edit, estate-region, preferences, land-parcel, inventory,
  teleport, appearance-wearables, money-economy, groups, friends-people,
  snapshot-social, diagnostics, login-session, avatar-movement, scripts,
  im-chat, media-sound, marketplace, voice, landmarks-navigation, misc,
  ui-hints, web-browser, security, experiences, premium-account, rlv), each
  owning its manifest rows and gated on that feature's raise-site plumbing.
