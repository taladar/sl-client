---
id: viewer-input-focus-contexts
title: Input focus / modal context state machine
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The "input focus" spine: a modal input-context state machine plus
focus-ownership routing via `bevy_input_focus`, deciding who receives input each
frame — the world, a UI widget, or a text field. Includes
**cursor-grab toggling** (the viewer today is unconditionally
`CursorGrabMode::Locked`, which is why no UI mouse interaction is possible); a
UI/text context releases the grab, a world/mouselook context takes it.

The context set covers **both** UI-focus states (Chat / TextEntry / Edit)
**and** the movement/camera modes (third-person / mouselook / sitting), which
mirror Firestorm's `keys.xml` **modes** — because a key binding is scoped to
whichever context is active ([[viewer-input-action-map]] holds the per-context
profiles). So this task owns *which context is active and who has focus*; the
action map owns *what a key does in that context*.

Reference (Firestorm, read-only): `indra/llwindow/llkeyboard` (focus/mode),
`llagentcamera` mode transitions, `llviewerwindow` focus handling.

Builds on: the current always-grabbed cursor in `main.rs`.
