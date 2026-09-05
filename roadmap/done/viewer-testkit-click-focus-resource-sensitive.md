---
id: viewer-testkit-click-focus-resource-sensitive
title: A click stops focusing its field when the harness gains any resource
topic: viewer
status: done
origin: viewer-floater-interaction-tests (2026-09-05) — hit while adding a marker
points: 2
refs: [viewer-ui-widget-interaction-suite, viewer-ui-interaction-contracts]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-testkit`'s `interact::tests::a_click_focuses_the_field_it_lands_on`
failed — the click landed, the field was laid out at a real size, and
`InputFocus` was left empty — as soon as one more resource was inserted while
`install_text_editing` built the app.

## What it actually was (2026-09-05)

**The resource was a red herring.** The variable is the field's **entity id**.
Padding the world with empty entities before spawning the field and sweeping the
click across eight ids gives a clean period-4 pattern: four ids in eight focus
the field, four leave `InputFocus` empty. A resource was merely one of the
things that moved the tie; the filed guess (`ComponentId` allocation) named the
wrong mechanism.

The cause is that **a fixture node spawned with no parent is a second UI root**:

- `text_field` (and the other fixtures in `interact.rs`) spawned a bare `Node`
  with no `ChildOf`, so it was a *sibling* of `spawn_ui_root`'s `UiRoot`, not a
  child of it. Two sibling roots have no defined order in the UI stack.
- The scaffold's root is deliberately `Pickable { should_block_lower: false }`
  so a click on empty UI space reaches the world behind it. So when the fixture
  sorted *behind* the root, the pointer hit **both**, and when it sorted in
  front, the root was blocked out and only the fixture was hit. Which way it
  went was the hover map's `EntityHashMap` order — hence the period in the
  entity id.
- With both hit, `bevy_input_focus`' `click_to_focus` raises one `AcquireFocus`
  per hit. The field's finds a `TabIndex` and focuses it; the scaffold root's
  finds none, bubbles to the window, and `acquire_focus` **clears the focus the
  field just gained**. `FocusGained(field)` immediately followed by
  `FocusLost(field)` in the trace, and `InputFocus` empty at the end.

So the harness was not order-sensitive in some diffuse way — it was standing on
a tree the viewer never builds, and the coin flip decided whether that showed.

## What landed

- `crate::spawn_under_root`, which spawns a fixture node as a child of the
  scaffold's `UiRoot` (running one frame first if the root is not up yet, since
  it is spawned at `Startup`). Every fixture in `interact.rs`'s tests now uses
  it. The registry's own fixtures never had the bug — `spawn_element_into` has
  always spawned under `UiRoot` — so this gives a hand-built fixture the same
  guarantee.
- `crate::orphan_root_violations`, in `interaction_violations`: a `Node` with no
  parent that is not the scaffold's root is reported. It is in the *interaction*
  list rather than the layout one because a pure layout fixture has no
  hit-testing for the second root to disturb. All 292 `sl-client-bevy-viewer`
  lib tests pass with it live, which is the evidence that the production
  fixtures were already clean.
- `a_click_focuses_the_field_at_any_entity_id`: the sweep itself, eight ids,
  all of which must focus. It has to stay a sweep — a single app cannot tell
  "focus works" from "this run drew a lucky id", which is exactly how the
  original test passed for months.
- `a_parentless_fixture_node_is_a_violation`: the tooth for the check.

Verified the original symptom too, not just the substitute: with the exact
`init_resource` back in `install_text_editing` the interaction tests pass, and
then it was removed again.

## Does the live viewer ship it?

The entry asked and nobody had looked. **Not in this shape**: `spawn_ui_root`
makes exactly one root and every panel, floater and widget is spawned into it,
so the viewer never has two sibling roots to tie. `orphan_root_violations` is
what keeps that true.

A *narrower* relative of it does exist live, and is filed separately as
[[viewer-nonblocking-overlay-steals-focus]]: a node that is
`Pickable { should_block_lower: false, is_hoverable: true }` and sits **in
front** of a focusable node that is not its own descendant produces the same
two-hit race, and the same window-bubbled `AcquireFocus` clears the focus. The
wrappers that carry that `Pickable` today (`nearby-chat-bar`, `volume-cluster`)
are *ancestors* of their focusables, so they are behind them and safe;
`notification-channel` is the one that is lifted above other panels by a
`GlobalZIndex`.
