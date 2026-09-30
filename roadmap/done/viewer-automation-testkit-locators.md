---
id: viewer-automation-testkit-locators
title: The in-process tiers speak locators too
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 3
blocked_by: [viewer-automation-locator-engine, viewer-automation-app-builder]
refs: [viewer-ui-interaction-harness, viewer-world-test-harness]
---

Context: [context/automation.md](../context/automation.md).

`InteractionTest`, `WorldTest` and `ViewerHarness` address widgets by `Name`
string (`click_node`, `text_of`, `centre_of`). Once the semantic model and
locator engine exist, the cheap tiers can use the same vocabulary as the
end-to-end tier, so a test moves down a tier without being rewritten.

## Wanted

- Testkit helpers that resolve a `Locator` in an `&mut App` (`locate`,
  `click_locator`, `expect_disabled`, …) through the same engine, with the
  same actionability checks.
- `ViewerHarness` (now built by `ViewerAppBuilder`, with its UI groups)
  gains the same helpers.
- Convert a handful of existing `Name`-string tests as the worked example,
  including one that asserts a disabled button does nothing.

Acceptance: the converted tests pass; an actionability failure in the
testkit reads the same as one in the end-to-end tier.

## Done (2026-09-30)

- **`sl_viewer_automation::in_app`**: `locate`, `find`, `text`, `click`,
  `hover`, `fill`, `press`, `expect` (+ `expect_disabled` / `_enabled` /
  `_hidden` / `_visible`) and `click_while_disabled`, each one request to
  the `&mut App`'s own executor (installed on first use), the app stepped
  until it answers. Failures are the driver's `DriverError::Failed`, so they
  print exactly as the end-to-end tier's (`viewer app: click [test_id=off]
  failed: timed out …`); frame-counted deadlines by default
  (`in_app::Options`). Ids from `IN_APP_ID_BASE` (2⁴⁶), below both transports'.
- **Not in `sl-viewer-testkit`**: the automation crate dev-depends on the
  testkit, and the UI crates it depends on cannot dev-depend back on it
  without a second copy of themselves, so the helpers live in the
  automation crate, for the viewer crate and the crates above it.
- **`ViewerHarness`**: `click`, `hover`, `expect` (wall-clock deadline, label
  `harness`); the off-screen-window test now opens the inventory through
  them instead of a hand-driven `Pursuit`.
- **Converted** (`build_floater_test`): every field edit (`fill` + `Enter`,
  replacing ~90 lines of hand-rolled scroll-into-view and caret checks), the
  tool radio, the toggle rows (tick read as the model's `checked`), the
  linked-part row (`expect_hidden` / `expect_visible` and its buttons) and
  the group picker round trip.
- **Disabled does nothing**: `a_greyed_group_set_button_does_nothing` presses
  the greyed Set… with the real pointer, sees no picker, then selects a prim
  and sees the same locator enabled and working. Its first run found a bug:
  the Build floater's parameter controls were **enabled with nothing ever
  selected** — `sync_param_widgets` redrew only on a changed snapshot, and
  "nothing selected" is `None` both before the first draw and after. Fixed:
  it also redraws when gated widgets are newly spawned.
