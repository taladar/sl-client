---
id: viewer-automation-world-model
title: World locators — find and read objects and avatars
topic: viewer
status: blocked
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-protocol]
refs: [viewer-world-test-harness, viewer-automation-world-aim]
---

Context: [context/automation.md](../context/automation.md).

A test about the world needs to say "the prim named *Door*", "Avatar Two",
"my own avatar" or "the nearest tree" and read its state.

## Wanted

- `WorldLocator` and `WorldNode` in `sl-automation-proto` (this task is
  their first consumer): object / avatar / self / attachment, by name, full
  id, local id, owner, pcode, proximity to a point or to the own avatar,
  hover text, with `nearest` / `nth`.
- Resolution over the viewer's own tracking: `ObjectState`
  (`sl-viewer-world-api/src/object_graph.rs`), `AvatarState` and
  `AvatarPickTarget` (`world_vocabulary.rs`), `SceneObject` /
  `ObjectDebugInfo`, attachment nodes, and the name-bearing
  `ObjectProperties` — requesting properties for candidates that have none
  yet, since an object's name arrives separately from its update.
- Readout: full id, local id, name, description, owner, pcode / kind,
  region-local position and rotation (from `GlobalTransform`, as the scene
  dump does), scale, parent / link set, attachment point, sit state,
  selected, hover text, name-tag text.

Acceptance: in a `WorldTest` fixture an avatar is found by name, a prim by
name once its properties arrive (and a query issued before they do waits
for them), and every readout matches the fixture.
