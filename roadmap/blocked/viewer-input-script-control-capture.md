---
id: viewer-input-script-control-capture
title: Script control capture (llTakeControls)
topic: viewer
status: blocked
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
