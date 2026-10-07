---
id: viewer-simulator-features-unread-keys
title: Act on the SimulatorFeatures keys the reference viewer reads and ours does not
topic: viewer
status: ready
origin: gridspec-simulator-features (2026-10-07)
refs: [gridspec-simulator-features, viewer-opensim-region-extras-limits]
---

Context: [context/viewer.md](../context/viewer.md).

`SimulatorFeatures` now carries a typed field for every key either live grid
sends (`book/src/gridspec/region-arrival.md` § SimulatorFeatures), and the
viewer reads five of them. The reference viewer acts on these as well
(`llviewerregion.cpp`, `llagentbenefits.cpp`, `lfsimfeaturehandler.cpp`):

- `RenderMaterialsCapability` — a rate: it spaces `RenderMaterials` requests
  to that many a second (4 on Second Life, 3 on OpenSim, 1 when absent). Ours
  sends every queued chunk at once.
- `MaxTextureResolution` — the largest texture it uploads and requests (2048
  on Second Life; absent on OpenSim, where it keeps 1024).
- `MeshUploadEnabled`, `MeshRezEnabled` — gate the mesh upload entry and mesh
  rendering.
- `PhysicsShapeTypes` — gates the build tool's physics-shape choice.
- `AnimatedObjects`, `MaxAgentAttachments` — the animesh triangle limit, and
  the attachment limits on a grid whose login names no benefits (OpenSim).
- `AvatarHoverHeightEnabled` — gates the hover-height control.
- `BakesOnMeshEnabled`, `PBRTerrainEnabled`, `PBRTerrainTransformsEnabled`,
  `PBRMaterialSwatchEnabled`, `GLTFEnabled`, `DynamicPathfindingEnabled` —
  each gates the feature it names. Second Life sends `GLTFEnabled` as
  `false`, which is not the same as OpenSim not sending it.
- `HostName` — the simulator host the About window names; ours shows the
  socket address.
- `OpenSimExtras.MinSimHeight` — the lowest altitude the map and the tracker
  accept; `SimulatorFPS` with `SimulatorFPSFactor` and the two percentages —
  how the statistics floater scales and colours an OpenSim region's frame
  rate.

The prim-scale limits, `ExportSupported` and `GridURL` are already
[[viewer-opensim-region-extras-limits]].

For each: find whether the viewer has the feature the key gates, wire the
key where it does, and say so here where it does not yet.
