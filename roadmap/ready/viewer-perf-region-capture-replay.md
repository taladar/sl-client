---
id: viewer-perf-region-capture-replay
title: Capture a whole region and replay it as a rendering benchmark
topic: viewer
status: ready
origin: Tracy check after the Bevy 0.20 upgrade (2026-11-22)
refs: [viewer-perf-arrival-timeline-replay, viewer-perf-render-app-bound-frame]
---

Context: [context/viewer.md](../context/viewer.md).

## Goal

Compare rendering performance before and after an optimization on identical
input. Live sessions cannot do that: the scene, the other residents and the
window's visibility all change between runs. Avatar replay is repeatable, but
holds only avatars and their attachments.

## Capture

- A timeline of the session events that shape the scene: objects, terrain,
  region identity, environment, appearances, animations and materials. It is
  recorded from login, because it cannot be reconstructed afterwards.
- Every mesh, texture, animation and material those events reference, in the
  drop-in cache layout avatar bundles use.
- A record of anything the capture could not obtain. A capture only holds what
  the grid sent, which depends on draw distance and camera position.

Captures hold other residents' assets. They stay on the machine that made them
and are never committed or shared.

## Replay

- Apply the timeline's final state, wait until assets settle, then measure a
  fixed number of frames headless.
- Freeze the environment at one time of day, and run animations on a fixed
  clock.
- Render from saved viewpoints or a scripted camera path.
- Switch layers on and off (world, avatars, both) to attribute frame cost.

## Comparison

A script takes two viewer binaries, alternates runs, and reports p50, p95 and
p99 frame times from the `finished frame` messages in the settled window.

## Progress

- [x] Timeline format and recorder (`SL_VIEWER_SCENE_CAPTURE_DIR`).
- [x] Bundle the referenced assets and report gaps (Ctrl+Alt+Shift+B, or
  `SL_VIEWER_SCENE_CAPTURE_SECONDS`). It must run before logout, because a
  region's capabilities end with the session.
- [x] Bundle a real region: an Aditi recording holds all 1,202 meshes and 73
  animations, and lacks 43 of 2,669 assets.
- [x] Retry with backoff, resume a bundle's gaps from a later session
  (`SL_VIEWER_SCENE_CAPTURE_RESUME`), and announce the outcome in nearby chat.
- [x] Resume that Aditi recording. It recovered one texture. 32 textures and a
  PBR material still answered 503 after a minute of retries, and the region
  returned none of the 9 legacy materials.
- [x] Steady-state replay of the timeline (`--replay <recording>`): its final
  state goes through the normal render systems and the scene settles.
- [x] Resume the Aditi recording for its 15 tree and grass textures. The first
  bundle did not know plants take theirs from a species table.
- [x] Closing the viewer cancels a running bundle and waits at most 5 s.
- [ ] Frozen environment, fixed animation clock, camera path.
- [ ] Layer switches.
- [ ] A/B comparison script.

A distant teleport during a recording is not handled yet: the timeline does not
carry `RegionChanged`, so replay would not purge the earlier region.
