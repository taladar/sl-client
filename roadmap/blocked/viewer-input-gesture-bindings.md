---
id: viewer-input-gesture-bindings
title: Gesture key bindings (input↔gesture bridge)
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-input-system
blocked_by: [viewer-input-action-map, viewer-gesture-runtime]
---

Context: [context/viewer.md](../context/viewer.md).

Bind an inventory **gesture** to a key. This is the input-side bridge: it uses
the **dynamic binding-target** support in [[viewer-input-action-map]] (a binding
may target a gesture, not only a fixed action) so that pressing the bound key
fires the gesture through the gesture runtime.

Blocked on [[viewer-gesture-runtime]] — the gesture runtime that actually
sequences and plays a gesture must exist first; the `blocked_by` records that
ordering. Once the runtime is fleshed out, this task wires the key →
gesture-fire path and its per-gesture binding row in the rebinding UI.

Reference (Firestorm, read-only): gesture hotkey binding, `llgesturemgr`.
