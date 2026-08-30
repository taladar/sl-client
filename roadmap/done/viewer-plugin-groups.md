---
id: viewer-plugin-groups
title: One plugin-group definition shared by the viewer, the readback rig and the headless harnesses
topic: viewer
status: done
origin: test-harness plan (2026-08-30) — the "one real unknown" every harness task names
points: 5
refs: [viewer-world-test-harness, viewer-render-readback-tier, viewer-ui-interaction-harness]
---

Context: [context/testing.md](../context/testing.md).

`lib.rs::run_session` assembles ~70 plugins inline, and
`render_readback.rs::build_readback_app` re-lists a subset by hand — so
the readback rig silently lacks sky, water, fog, exposure, tonemap, glow,
local lights, particles and name tags, and a headless fixture world cannot
be built at all. Carve the assembly into six `PluginGroup`s in
`sl-client-bevy-viewer/src/viewer_plugins.rs`, moved verbatim with their
comments:

- `ViewerUiPlugins` — scaffold, i18n, widgets, panels, floaters, toolbar,
  notifications.
- `ViewerInputPlugins` — input context/actions, camera, movement, sit
  camera, spacenav.
- `ViewerWorldPlugins` — a new `WorldIngestionPlugin`
  (`world_plugins.rs`: the inline ingestion systems and resources,
  byte-for-byte), HUD screen, physics, raycast index, `PickRegistryPlugin`,
  avatar/animesh, object diagnostics/cost, render-layer propagation,
  world-scoped state, the four world pie menus, `PieMenuCorePlugin`.
- `ViewerEditPlugins` — every `sl-viewer-edit` plugin.
- `ViewerRenderPlugins` — material plugins, `GpuPickRenderPlugin`,
  `PieMenuRenderPlugin`, billboards, particles, sky, water, exclusion,
  local lights, transparency, water clip, underwater fog, exposure,
  tonemap, glow, probes, shadow visibility, GPU avatars
  (`GpuAvatarsPlugin::default_mode()` beside `from_env()`), minimap/map.
- `ViewerShellPlugins` — the client config, audio, CEF/media, web auth,
  clipboard, persistence, snapshot, screenshot, diagnostics.

Two plugins split so a headless app takes the ECS half without the shader
half: `GpuPickPlugin` → `PickRegistryPlugin` + `GpuPickRenderPlugin`;
`PieMenuPlugin` → `PieMenuCorePlugin` + `PieMenuRenderPlugin`.

Also extract the main camera's component bundle (`lib.rs::setup_scene`)
into `viewer_camera_bundle(transform)` in `sl-viewer-world-view` — fog,
exposure, tonemap and glow select the view by those components and are
silently inert on the readback camera today. `SceneRuntimePlugin` must
not double-register the particle drivers when `ParticlesPlugin` is
present; move `simulate_flexi`/`drive_texture_animations` into plugins
both include once.

Acceptance: the `lib.rs` diff is a pure move; a `--screenshot-dir` smoke
run against the local grid and the settings golden are unchanged;
`render_readback` builds on `ViewerRenderPlugins` and stays green.

Done (2026-08-30): `viewer_plugins.rs` holds the four groups (Input,
Render — `RenderStack::{Full, Bare}` —, World, Edit), the main camera is
`viewer_camera_bundle` in `sl-viewer-world-scene`, the readback rig runs
`ViewerRenderPlugins::bare()`, and a fixed-camera before/after screenshot
of the local grid differs only in the animated sea and the clock.
