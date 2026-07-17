---
id: viewer-render-scene-coverage
title: Render-scene coverage — a scene per render path the viewer already has
topic: viewer
status: ready
origin: the viewer-render-test-harness work (2026-07); the harness shipped with 14 scenes against a viewer that renders far more than 14 things
blocked_by: [viewer-render-test-harness]
refs: [viewer-render-test-harness, viewer-render-readback-tier]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-render-test-harness]] built the mechanism and seeded a registry. The
registry is **not** the point — the checks × scenes product is, and today the
scene half is the short one.

This is the difference from [[viewer-ui-test-harness]], and it is worth stating
plainly because it inverts that task's reasoning. The UI registry seeded
*patterns* for panels that did not exist yet: there were no viewer panels to
register, so the elements were the vocabulary the future ones would be built
from. Rendering is the opposite.
**The viewer already renders nearly everything** — terrain, water, sky, prims,
sculpts, meshes, rigged meshes, avatars, trees, grass, particles, flexi, HUD,
lights, probes, post-processing — and every one of those paths is unscened,
which means every check in the harness runs against a fraction of the code it
could.

Each scene below is cheap (the mechanism exists; a scene is a `spawn` fn and a
registry line) and each one multiplies: a scene added here inherits every check
that exists now and every check added later, at every LOD, at every sample.

## The paths with no scene

Roughly in order of how much they have already cost in `R*` bugs:

- **Avatar, properly.** The one scene today (`avatar-base-part`) is a single
  decoded `.llm` on no skeleton. The R1 / R13 / R22 cluster lives in the parts
  it does *not* reach: the real skeleton (`BevySkeleton`, `base_mesh_skin`), the
  morph bake (`to_bevy_morphed_mesh`), the runtime morphs, joint overrides, the
  multi-part body with its alpha layers. Needs `SL_VIEWER_ASSETS`, so it is a
  scene that skips when the Linden `character/` dir is absent — which is a real
  cost and worth paying only for this one.
- **Terrain.** `terrain.rs`'s `build_patch_mesh` is already a pure
  `(patches, composition) -> Option<Mesh>` and already has six tests; a scene
  would fold it into the sweep and cover the region-edge / neighbour-offset
  cases those tests do not.
- **Flexi.** `simulate_flexi` is a `Time`-driven mesh rebuild — a natural
  **dynamic** scene, and the second one after particles. The chain's settle is
  exactly the "declares a timeline, must change" shape.
- **Texture animation.** `drive_texture_animations` rewrites a material's
  `uv_transform` per frame: dynamic, and a scene would catch a frame that
  animates off the end of its atlas.
- **Legacy materials / bump.** Both set a sampler (the R22h path
  `sampler_violations` guards) and both build normal maps; neither has a scene,
  so the sampler check currently only sees the one textured prim.
- **Sky, stars, sun disc, clouds, water.** Custom materials with their own
  shaders. The geometry tier says little about them; they are mostly for
  [[viewer-render-readback-tier]] — but a scene is the prerequisite for either.
- **HUD.** `hud.rs` + `hud_pick.rs` render on their own layer with an
  orthographic camera; a HUD scene would pin the attachment-point geometry the
  `sl-client-opensim-hud-test-attachment` memory currently tests by hand.
- **Tree billboard / impostor.** `tree_billboard_geometry` is the distance LOD
  the `tree` scene never reaches.
- **Animesh / control avatars**, **body physics**, **IK / locomotion** — each
  time-varying, each currently verified only by a login.

## What to watch for

- **Do not let a scene need a session.** That is the registry's one rule, and
  each path above should be checked against it *before* the scene is written;
  where a spawn path reaches for live `ObjectState`, the fix is to separate the
  decode from the transport, not to fake a session.
- **A scene that needs an env var is a scene that silently skips.** Only the
  avatar earns that. Prefer procedural fixtures (the harness's own convention)
  and, where the input is genuinely an asset, the smallest one that reproduces
  the class.
- **Expect findings.** The harness's first honest run over 14 scenes found a
  fifth texture path that never set its sampler. Fourteen more paths is more of
  that.
