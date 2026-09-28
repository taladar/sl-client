---
id: viewer-automation-synthetic-input
title: Synthetic input for a live viewer — interact.rs as a frame-queued injector
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol]
refs: [viewer-ui-interaction-harness, viewer-ui-keyboard-text-harness,
  viewer-automation-windowless-mode]
---

## Done (2026-09-28)

The input core is `sl_viewer_ui_core::synthetic_input`, not a module of
`sl-viewer-automation` as planned (user decision, 2026-09-28). The testkit
has to depend on the core, and `sl-viewer-automation` depends on
`sl-viewer-ui-widgets` and `sl-viewer-ui-pie-menu`. Both of those
dev-depend on the testkit, so that home would have made dev-dependency
cycles: every widget crate's test build would compile its own library
twice, and its unit tests would see two copies of those types. `ui-core`
needs only `bevy` for this, both crates already depend on it, and it
already holds the semantic vocabulary. The executor takes the injector
from there.

- **`InputStep`** is one frame's input: move, press, release, wheel, raw
  motion, key down/up (physical key, logical key, text), an IME event
  (enable, preedit, commit, disable) or idle. **`InputAction`** is a list
  of steps. Its constructors are the gestures: `click` (move, press,
  release, settle, with single clicks pinned), `double_click`, `drag`,
  `scroll`, `mouse_motion`, `key_down` / `key_up` / `tap`, `type_text` (the
  US-layout physical key under each character's logical key, as before),
  `ime`, plus the new higher-level ones: `chord` (a menu accelerator:
  modifiers down in order, the key tapped, modifiers up in reverse),
  `hold_key` (a movement key held for N frames) and `mouse_look` (raw
  motion spread over N frames). `then` joins actions.
- **`SyntheticInput`** (a resource) queues actions FIFO and returns an
  `InputActionId`. `SyntheticInputPlugin` drains it. In `First`, after the
  message buffers swap and **before `PickingSystems::Input`** (picking turns
  window events into pointer events in `First` too; written after it, a
  press reached the pointer a frame late, and `ui_tab`'s held-arrow test
  caught a held button stepping once more after its release), it applies
  one step. In `Last` it marks an action
  whose steps are spent as `Done { frame }`, in its own frame counter, and
  gives back the multi-click interval if the action pinned it. That is the
  same moment the testkit used to restore it. An action with no steps
  finishes without taking a frame. Completions are kept for the last 1024
  actions; older ones report `Expired`.
- **The testkit is a wrapper.** `interact::perform` queues an action and
  runs `update()` until it is done. Every old function (`hover`, `press`,
  `click`, `drag`, `scroll`, `type_str`, `ime_*`, …) calls it, with the
  same frame counts as before. `install_input_stack` installs the plugin,
  and `key_code_for` is re-exported.

Tests: the core's own suite (`synthetic_input/tests.rs`: one step per
frame and done at the last, order across actions, empty actions, a held
key pressed for exactly its frames, chord order, the pin held during a
click and restored at its last frame, cursor moves, mouselook totals,
expiry). The testkit's interaction and keyboard suites pass unchanged on
the moved core. A new testkit test has a system inside a running app queue
a click at a frame of its own choosing: the app only runs its frames and
the button fires once.

Not done here: the live window's own consequences. Setting the window
cursor under winit warps the real pointer, and real winit input mixes with
synthetic input. Isolating those belongs to
[[viewer-automation-windowless-mode]].

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
