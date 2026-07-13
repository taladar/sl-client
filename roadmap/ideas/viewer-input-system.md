---
id: viewer-input-system
title: Input system (rebindable keys + script capture)
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-preferences-ui, viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

A real input-mapping layer replacing today's hardcoded keys: named actions with
user-**rebindable** bindings (keyboard + mouse), conflict detection, persisted
config, and modal contexts (walk / fly / mouselook / edit / text-entry) that
switch which bindings are live and who owns focus.

Critically, support **script key capture** — `llTakeControls` / control-
permission grants route the captured keys (forward / back / left / right / up /
down / etc.) to the object and withhold them from normal movement until the
script releases them. This ties into the permission-request handshake.

This is foundational: it defines focus and control routing for every UI
component built on top, which is why it sits with the UI/i18n foundations near
the start.

Reference (Firestorm, read-only): `llviewerinput.cpp/h` (keybinding table,
`keys.xml`), `llkeyconflict`, `indra/llwindow/llkeyboard`, and `llagent`
control-flag forwarding for `AgentUpdate`.

Builds on: `movement.rs` / `camera.rs` (currently fixed keys) and the permission
protocol for the control-grant handshake.

Deps: [[viewer-preferences-ui]] (the rebinding UI), [[viewer-ui-framework]].
