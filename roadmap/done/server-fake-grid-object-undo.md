---
id: server-fake-grid-object-undo
title: Fake grid — keep an object's edit history for Undo / Redo
topic: server
status: done
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-live-verify-sweep, gridspec-object-edit,
  test-fake-grid-edit-surfaces]
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

## Done (2026-10-10)

The history itself came with `test-fake-grid-edit-surfaces`, as whole
snapshots of an object ten deep. [[gridspec-object-edit]] measured what the
live grids keep and made it that: how an object was placed before each
move, turn and resize and nothing else, twenty deep on a
Second-Life-flavoured grid and five on an OpenSim-flavoured one, answered
with a terse update or — where a size is restored — a full one. The
`e2e_objects` edit test moves a prim through the Build window, presses
Ctrl+Z and reads the Z field return to its old value, and Ctrl+Y for the
new one, on both flavours.
