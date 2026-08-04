---
id: viewer-preferences-alerts-tab
title: Preferences — alerts / popups tab
topic: viewer
status: done
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-preferences-floater]
refs: [viewer-ui-notification-host]
---

Context: [context/viewer.md](../context/viewer.md).

The **alerts** tab: per-notification enable/disable — the two lists the
reference shows ("always show" / "never show") over every suppressible
notification the host ([[viewer-ui-notification-host]]) registers, driven
by a per-notification `show_again` flag in the settings store, plus the
headline toggles (friend online/offline notices, group notice toasts,
inventory-offer auto-accept behaviour). Requires the notification host to
expose its registry of notification ids + descriptions — add that hook
there when this lands.

Reference (Firestorm, read-only): `panel_preferences_alerts.xml`,
`llnotifications` (ignore settings).

Deps: [[viewer-preferences-floater]].

## Done

New viewer module **`src/preferences_alerts.rs`** (`PreferencesAlertsPlugin`,
tab id `alerts` in `PREF_TABS`), plus the ignore-kind port underneath it.

- **UI choice (user decision):** Firestorm's *current* single filtered
  checkbox list (`buildPopupList` over `all_popups`) — one virtualized
  `ui_table` row per suppressible template ("Show" checkbox + label) — not
  the older two-list + move-buttons design this task file described (that
  code is commented out upstream). The floater's shared search box doubles
  as the list filter (match against the resolved labels; the tab reports
  its hits through the new `PreferencesExtraHits` resource so the shell's
  dim / first-hit-jump treats the virtualized list like any row).
- **Ignore kinds ported** (user decision): `ignorable: bool` became
  `ignore: NotificationIgnore` (None / CheckboxOnly / DefaultResponse /
  DefaultResponseSessionOnly / LastResponse / ShowAgain — CheckboxOnly is a
  deliberate addition beyond the plan's five, the reference
  `IGNORE_CHECKBOX_ONLY`) + `ignore_key` (the ported reference
  `ignoretext`, 142 new Fluent strings) across all 1308 templates, by
  scripted extraction from the reference `notifications.xml` (`ignoretext=`
  usetemplate attribute or inline `<ignore>`). Current census, pinned by
  test: 137 DefaultResponse, 3 LastResponse (`ReplaceAttachment`,
  `FirstJoinSupportGroup2`, `DoNotDisturbModePay`), 2 CheckboxOnly
  (`ParcelPlayingMedia`, `PromptMFATokenWithSave` — excluded from the list,
  per the reference `ignore > IGNORE_NO` criterion), 0 session-only /
  show-again.
- **A suppressed raise now auto-responds** instead of silently dropping:
  default-response kinds fire the form's default button (a suppressed
  confirmation still proceeds), LastResponse replays the persisted last
  button (`Default<name>` String setting; saved when the "always choose
  this option" box rides a button press), ShowAgain stays mute.
  Session-only suppressions register as transient settings (never
  persisted). The toast checkbox label is now kind-aware (plain /
  session-only / "always choose"; a CheckboxOnly template's label is its
  own ignoretext).
- **Registry hook:** satisfied by the existing `NOTIFICATIONS` slice +
  `register_notification_settings` (one account-scope `Bool` per
  suppressible template, key = template name) — the tab binds those
  settings directly; hidden widgetless `SettingBinding` markers keep the
  whole list inside the shell's Cancel/OK snapshot though only on-screen
  rows materialise.
- **Headline toggles** (all greenfield, account scope, `[notifications]`
  section): `ChatOnlineNotification` (reference name; new
  `notify_friend_presence` in `people.rs` raises the `FriendOnlineOffline`
  tip per presence change, unique-per-agent context),
  `ShowGroupNoticeToasts` (our name — the reference has no single global
  gate; gates the card *and* the relogin persist, since a persisted notice
  would re-raise a card and defeat the setting; notices stay readable in
  the group's server-side Notices tab), `AutoAcceptNewInventory`
  (reference name, default off; files offers silently into the type
  folder, falling back to the card when the destination cannot resolve
  yet — an offer is never dropped). A `ShowNewInventory`-style "opened
  what you received" notice is deferred.
- Debug affordance `SL_VIEWER_PREFERENCES_TAB=<tab id>` (the
  `SL_VIEWER_UI_DEMO` idiom) selects a preferences tab once the shell
  builds, so the offline screenshot harness can land on a chosen tab.
- **Virtual-list scrollbar** (found live on this tab, fixed for every
  list): virtualized viewports had no scrollbar at all — `virtual_list`
  now owns a slim overlay bar (thumb proportional to the visible
  fraction, draggable, hidden while content fits; driven from
  `VirtualList` directly since Bevy's `Scrollbar` drives the native
  `ScrollPosition` a virtual list does not use), spawned automatically by
  `spawn_table`, so the friends / group / land tables gain it too.
- **Wheel-over-list fix** (found live on this tab): the wheel over a
  not-yet-clicked list did *nothing* — `scroll_virtual_lists` waited for
  a click-based focus flip while the camera zoom already stood down over
  blocking UI. The wheel now scrolls the hovered virtual list purely by
  hover; the camera's existing `pointer_over_blocking_ui` guard is the
  other half of the coordination.

Verified by unit tests (ignore-kind census port-lock, ignore-key ⇔ kind
coherence + Fluent coverage, `auto_response_button` over all kinds incl.
saved-last-response fallback, resolve-path writes per kind, the
widgetless-marker snapshot revert, the list's pure filter/sort) — 915
green — plus the extended gallery specimen and live checks on the local
grid: the alerts tab's layout, defaults and per-row toggles
(screenshot-verified; scrollbar + wheel user-confirmed), and the friend
online / offline tips observed both ways (the test avatars are now
friends with see-online rights — seeded directly in the grid's
`friends.db`, grid state not git). The suppressed-confirm auto-respond,
group-notice gate and inventory auto-accept are covered by the unit
tests, not exercised live.
