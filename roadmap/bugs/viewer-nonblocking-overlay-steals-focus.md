---
id: viewer-nonblocking-overlay-steals-focus
title: A see-through container in front of a field clears its focus, at random
topic: viewer
status: bugs
origin: found while fixing [[viewer-testkit-click-focus-resource-sensitive]]
  (2026-09-05)
points: 3
refs: [viewer-testkit-click-focus-resource-sensitive]
---

Context: [context/viewer.md](../context/viewer.md).

A node carrying `Pickable { should_block_lower: false, is_hoverable: true }`
that sits **in front of** a focusable node which is not its own descendant makes
clicking that node focus it only about half the time. The other half the click
lands and does nothing: the field takes focus and loses it again in the same
frame.

## Why

`bevy_picking` walks the hits front to back and stops at the first blocker, so a
non-blocking-but-hoverable node in front leaves **two** entries in the hover map
— the overlay and the control under it. `bevy_input_focus`' `click_to_focus`
then raises one `AcquireFocus` per hit (it fires once per hit entity, gated on
`press.entity == press.original_event_target()`, not once per click):

- the control's finds its `TabIndex` and focuses it;
- the overlay's finds no `TabIndex`, bubbles all the way to the window, and
  `acquire_focus`'s window arm calls `focus.clear()`.

Whichever lands last wins, and the order is the hover map's `EntityHashMap`
iteration order — a function of entity ids, so it is a coin flip that is stable
within a run and moves whenever anything upstream spawns a different number of
entities. This is the same clearing arm that produced
[[viewer-testkit-click-focus-resource-sensitive]]; that one was a harness
fixture shaped like nothing the viewer builds, this one is a shape the viewer
does build.

## Where it can bite today

`grep` for `should_block_lower: false` across the viewer crates. Most of them
are safe and worth keeping that way:

- `nearby-chat-bar` (`nearby_chat_bar.rs`) and `volume-cluster`
  (`volume_panel.rs`) are **ancestors** of their own focusables, so they render
  behind them and are blocked out of the hover map by their own children. A
  click on their empty part correctly reaches the window and hands the keyboard
  back to the world, which is what the non-blocking `Pickable` is for.
- the minimap's mouselook transparency (`minimap.rs`) pairs
  `should_block_lower: false` with **`is_hoverable: false`**, which keeps it out
  of the hover map entirely. That is the pattern the others should follow.
- `notification-channel` (`notification_host.rs`) is the one that is genuinely
  exposed: it is `is_hoverable: true`, non-blocking, and lifted above the normal
  stack by `GlobalZIndex(TOAST_CHANNEL_Z)`, so it is in front of other panels
  rather than behind its own children. A focusable widget under the channel's
  box — the gap between two stacked toasts, say — is in the race.

## Fix

Two halves, and the first is cheap:

1. **The channel does not need to be hoverable.** Its hover observers are on the
   *toasts* (`Pointer<Over>` / `Pointer<Out>` in `notification_host.rs`), not on
   the container, so `Pickable::IGNORE` — or at least `is_hoverable: false` —
   takes it out of the hover map and out of the race. Check the overflow control
   still picks (it is a child, so it should).
   **Note:** `sl-viewer-notices` was claimed by another worktree when this was
   filed; coordinate before editing it.
2. **The general shape should not be able to recur.** Only the topmost hit has
   any business deciding focus, so `click_to_focus` raising an `AcquireFocus`
   for a hit that is *not* the front-most one is arguably the upstream bug —
   `acquire_focus`'s window arm then reads "the user clicked empty space" from a
   click that was not on empty space at all. Worth a fork change in
   `taladar/bevy` (which this workspace already pins) plus an upstream PR, the
   same way [[viewer-widget-any-mouse-button-activates]] went.

A harness check in the shape of `crate::orphan_root_violations` would be the
third half: a fixture where a non-blocking hoverable node overlaps a focusable
one that is not its descendant is reportable without clicking anything.

## How to verify

The harness can do it without a grid: put a focusable field under a node with
`Pickable { should_block_lower: false, is_hoverable: true }` and a higher
`GlobalZIndex`, then sweep the field's entity id the way
`a_click_focuses_the_field_at_any_entity_id` does. Half the ids will miss.
