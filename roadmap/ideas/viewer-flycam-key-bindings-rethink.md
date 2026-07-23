---
id: viewer-flycam-key-bindings-rethink
title: Rethink the flycam mode's key bindings
topic: viewer
status: ideas
origin: user request during the edit-gizmo session (2026-07-23)
refs: [viewer-input-rebinding-persistence]
---

Context: [context/viewer.md](../context/viewer.md).

The flycam binding profile needs a deliberate redesign rather than more
point fixes. During the edit-tools work the bare-modifier movement keys
were removed (`Ctrl` → down collided with chord shortcuts like `Ctrl+B`,
and `Space` → up went with it; `E`/`C` + `PageUp`/`PageDown` stand in for
now), but the whole profile — WASD, vertical motion, speed modifier, what
`M` does, how it relates to the third-person profile — should be thought
through as a set, together with the rebinding UI / persistence tasks that
will expose it.

Builds on: `input_action.rs` (the per-mode `BindingProfile`s).
