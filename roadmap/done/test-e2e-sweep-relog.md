---
id: test-e2e-sweep-relog
title: End-to-end tests for what must survive a relog
topic: test
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
points: 5
refs: [test-e2e-live-verify-sweep, test-e2e-stage]
---

Context: [context/automation.md](../context/automation.md).

The pending live checks whose point is *after a relog*: a stage viewer logs
out and back in within one test, keeping its directories. The stage has no
relog yet; adding one (`Stage::relog(label)`: log out, start a new App or
process on the same `ViewerPaths` root, wait until it settles) is the first
part of this task, and each test below uses it.

- [[viewer-ui-floater-persist-geometry]]: move and resize the inventory,
  relog, and the geometry is restored.
- [[viewer-notification-persistence]]: a notice left unanswered comes back
  after a relog; answered, it is gone after the next.
- [[viewer-derender-blacklist]]: a derendered object stays derendered
  after a relog; a temporary entry clears on a teleport.
- [[viewer-contact-set-presence-extras]]: the settings window's checkboxes
  and reply editors persist, and the online-first order holds.
- [[viewer-preferences-alerts-tab]]: a suppressed confirmation auto-responds,
  the group-notice gate and inventory auto-accept hold across a relog.

## Done

`Stage::relog(label)` (`sl-e2e`): the viewer is asked to log out as the
teardown asks it, must exit having done so and — on the fake grid — leave no
session; a new App or process then starts on the same directories, logs in
where the stage's viewers start (on aditi after its cooldown turn) and is
waited for as at the start. It answers the new session's `Viewer`, which
`Stage::viewer` answers from then on, so `Stage::viewer` now hands out an
owned handle. A relogged process writes `viewer.<n>.log`.

`tests/e2e_relog.rs`, on both backends:

- **Window geometry**: the inventory, dragged by its title bar and resized
  by its grip (`UiLocator::drag_by`, a new `RequestBody::DragBy`), comes back
  where it was left and as big.
- **Notification persistence**: a group notice the grid posts comes back
  after a relog; answered, it does not after the next.
- **Derender**: a blacklisted object stays gone after a relog while a
  temporary one is back; a temporary entry clears on a teleport and the
  permanent one stays (the Asset Blacklist window's count and rows).
- **Contact sets**: a set made with New Set…, two friends filed from their
  profiles, the settings window's three ticks and the set's own Unavailable
  reply all persist, and the online friend stays above the one whose name
  sorts first.
- **Alerts tab**: Delete Set's confirmation unticked, group-notice toasts
  off and auto-accept on hold across a relog — the tab shows them so, Delete
  Set deletes without asking, a group notice raises no card, and an offer is
  accepted with nobody asked.

For the grid's side: `FakeGridBuilder::friends` (each friend in the other's
login `buddy-list`), every session naming every account through both
`GetDisplayNames` and `UUIDNameRequest` (the latter was never answered, so
the friends list showed ids), and in `sl-proto` the inverses of two decoders
— `GroupNoticeReceived::instant_message` and `InventoryOffer::binary_bucket`.

The tests found two viewer bugs, each fixed with a unit test:

- **A blacklisted object came back after a relog.** The blacklist is read
  once the account directory resolves, while the arrival burst may still wait
  in the scene mirror's backlog; the backlog drain only asked the
  region-scoped index, which had read those updates before the entry existed,
  and the purge found nothing in the scene. The drain now asks by full id and
  parent too, and records what it drops so Re-render can fetch it back
  (`DerenderList::suppresses_queued`).
- **Preferences' OK and Cancel were out of reach at 1280×720.** The window
  is 651 px tall and opened with its footer under the bottom controls. A
  window now opens wholly on screen — a resizable one taller than the room
  gives up content height to its floor (`fit_on_open`, the reference's
  `adjustToFitScreen`) — and the row of permanent controls on the bottom
  toolbar (chat bar, media, volume) is screen chrome, reserved through the
  toolbar it rests on (`snap_rect_of` credits a band stacked on a reserved
  one).
