---
id: server-fake-grid-object-undo
title: Fake grid — keep an object's edit history for Undo / Redo
topic: server
status: ideas
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-live-verify-sweep]
blocked_by: [gridspec-object-edit]
---

Context: [context/testing.md](../context/testing.md).

Object undo is the simulator's: the viewer's Undo / Redo only name the
selected objects, and the simulator reverts each from a bounded per-object
history (OpenSim's `SceneObjectPart.Undo()` / `Redo()`). The fake grid
decodes the messages into `ServerEvent::ObjectsUndone` / `ObjectsRedone` and
does nothing with them, so the end-to-end chord test can only check that
the grid heard the request, not that the prim moved back. A history of each
object's transform writes (position, rotation, scale — what OpenSim keeps),
stepped by the two events and re-streamed as an `ObjectUpdate`, would let
that test read the X field return to its old value.
