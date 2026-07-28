---
id: viewer-ui-notification-host
title: Notification / toast host
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-ui-framework/viewer-notifications-dialogs
blocked_by: [viewer-ui-widget-scaffold, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The toast / notification **host**: the container surface that stacks, times out
and dismisses transient notifications, plus a notification list / history. This
is the shared substrate the specific dialogs sit in — the script permission
dialog ([[viewer-permission-request-dialog]]), the experience-acceptance prompt
([[viewer-experience-permission-dialog]]), and the remaining dialogs still
tracked by [[viewer-dialog-offers-invites]] (inventory / teleport /
friendship / group offers and notices).

Model the reference's **declarative notification catalogue** (notification types
declared as data, not code). Styling comes from the [[viewer-ui-skin-tokens]]
tokens.

Reference (Firestorm, read-only): `llui/llnotifications`,
`llnotificationmanager`, `lltoast*`, `llnotification*handler`.

## Done

Two new viewer modules: `src/notifications.rs` (the declarative catalogue +
runtime state, pure and unit-tested) and `src/notification_host.rs` (the Bevy
plugin — rendering, timing, teardown).

- **Catalogue as data** — `NotificationTemplate` mirrors the reference
  `LLNotificationTemplate` (name, kind, message key, priority, persist,
  log_to_chat, unique, ignore, form of buttons); `NOTIFICATIONS` is a seed table
  covering all four kinds. `NotificationKind` (Tip / Notify / Alert /
  AlertModal) carries the reference timeouts (`NotificationTipToastLifeTime` 10
  s / `NotificationToastLifeTime` 30 s / `ToastFadingTime` 2 s). A `[KEY]`
  substitution engine and an `AlertInfo` `ExtraParams` parser reproduce the
  reference substitution.
- **Toast host** — a top-trailing screen channel, ageing by frame-time, fading
  over the fade window, hover-to-pause, priority-sorted order (highest / newest
  floats to the top); sticky alerts; modals over an input-blocking scrim. Each
  corner toast has a close × for early dismissal; only a capped number show at
  once, the rest queue (paused) behind a "N more ▸" control that cycles them
  into view (the reference notification well). Buttons + a "don't show again"
  checkbox wired to `NotificationResponse`; suppression persisted per-avatar
  (the hook the alerts tab needs). `log_to_chat` echoes the body into the
  nearby-chat overlay.
- **Live source** — surfaces the previously-unconsumed `AlertMessage` /
  `AgentAlertMessage` stream as notifications; `SL_VIEWER_NOTIFICATION_DEMO=1`
  raises a sample spread. A gallery specimen is registered in `ELEMENTS`
  (swept by `ui_test`), with `.sk-toast` skin classes and `en/main.ftl` keys.

The specific dialogs ([[viewer-permission-request-dialog]],
[[viewer-dialog-offers-invites]], [[viewer-dialog-lldialog]]), the history panel
([[viewer-notification-history]]) and the alerts tab
([[viewer-preferences-alerts-tab]]) build on this substrate. Per-notification
**sound** is the one reference attribute left unmodelled — it needs a UI-audio
consumer that does not exist yet.
