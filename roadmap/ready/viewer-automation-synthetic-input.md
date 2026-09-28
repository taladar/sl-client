---
id: viewer-automation-synthetic-input
title: Synthetic input for a live viewer — interact.rs as a frame-queued injector
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol]
refs: [viewer-ui-interaction-harness, viewer-ui-keyboard-text-harness,
  viewer-automation-windowless-mode]
---

Context: [context/automation.md](../context/automation.md).

`sl-viewer-testkit/src/interact.rs` already writes real input headlessly —
the typed `bevy_input` / `bevy_window` messages plus their `WindowEvent`
wrappers, one step per `update()` — but it is an `&mut App` API that steps
frames itself, so it works only where the test owns the loop. A live viewer
(or one behind a remote transport) runs its own loop.

## Wanted

- A new `sl-viewer-automation` crate with the input core moved out of the
  testkit: an `InputScript` queue (a resource) that a system drains **one
  step per frame** — move, press, release, wheel, drag path, key down/up,
  chords with modifiers, text, IME — preserving the frame protocol
  `interact.rs` documents (picking reads last frame's layout; a click is
  three frames; the multi-click interval is pinned around test clicks).
- Higher-level steps: menu accelerators, held movement keys for N frames
  (walk / fly / turn), mouse-look motion.
- Completion reporting: each queued action has an id and a "done at frame"
  so the executor can sequence and wait.
- The testkit's functions become thin wrappers that enqueue and step, so
  every existing interaction test keeps passing unchanged.

Acceptance: the testkit's interaction and keyboard suites pass on the moved
core; a system-level test enqueues a click from inside a running App (no
test-driven stepping between steps) and the widget fires.
