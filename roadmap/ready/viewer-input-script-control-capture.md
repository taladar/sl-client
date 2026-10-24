---
id: viewer-input-script-control-capture
title: Script control capture (llTakeControls)
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-input-action-map, viewer-permission-request-dialog]
---

Context: [context/viewer.md](../context/viewer.md).

Support **script key capture** — `llTakeControls` / control-permission grants
route the captured keys (forward / back / left / right / up / down / etc.) to
the object and **withhold them from normal movement** until the script releases
them. Route the captured controls through the action map
([[viewer-input-action-map]]) so it participates in the same per-context
resolution.

Gated on a `PERMISSION_TAKE_CONTROLS` grant — either from the permission dialog
([[viewer-permission-request-dialog]]) **or via an automatic grant** that
bypasses the dialog: an attachment worn by the agent, an object the agent sits
on, or a script running under an accepted experience
([[viewer-experience-permission-dialog]]). Capture must honour those auto-grants
without a prompt.

Reference (Firestorm, read-only): `llagent` control-flag forwarding for
`AgentUpdate`, the `ScriptControlChange` / take-controls handling in
`llviewermessage`.

Builds on: the existing permission-request protocol handshake and `movement.rs`
control-flag emission.

## Parity-audit addendum (2026-08-19)

Missing explicit scope: the reference also binds
`script_trigger_lbutton` — in mouselook (and via the per-mode LMB
binding rows in `key_bindings.xml`) the left mouse button must feed
ML_LBUTTON_DOWN/UP control bits to a controls-grabbing script instead
of performing a world click. This needs mouse buttons as binding
sources, so it pairs with [[viewer-input-mouse-button-bindings]].

## From gridspec-lsl-avatar-control (2026-10-24)

What each grid sends, measured by the `avatar/controls` probe
(`book/src/gridspec/lsl.md` § Controls) — the capture has to take both as
they come:

- **Second Life** opens a script's first `llTakeControls` with a
  `ScriptControlChange` releasing **every** control (`0xFFFFFFFF`; a later
  take releases what the last one took), then takes the keys plus the
  nudge-forward bit for `CONTROL_FWD` (it arrives as `0x80001`). With accept off
  (`llTakeControls(keys, FALSE, TRUE)`) it takes the **complement** of the
  keys (`0xFFFFFEFF` for turn-left), passed to the agent. `llReleaseControls`
  sends the release of what was taken.
- **OpenSim** releases just the named keys (passed and not), takes exactly
  the keys, takes the keys themselves for accept off, and sends **no**
  `ScriptControlChange` on `llReleaseControls`.
- Both drop `TAKE_CONTROLS` from the script's permissions on
  `llReleaseControls` and when a sitter stands up, and both deliver a held
  key as a press, repeats and a release (Second Life about 21 repeats a
  second, OpenSim one) from the plain `AgentUpdate` control flags.
