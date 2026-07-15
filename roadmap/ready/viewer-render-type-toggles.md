---
id: viewer-render-type-toggles
title: Render-type & render-feature toggles (hide object classes, wireframe)
topic: viewer
status: ready
origin: render-feature gap analysis vs Firestorm (2026-07)
refs: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

The quick "turn off whole categories of the scene" toggles that live in
Firestorm's Advanced / Develop menus and that builders and photographers use
constantly — hide the water to shoot what's under it, hide particles to cut a
laggy scene, drop everyone else's avatars for a clean landscape, flip to
wireframe to inspect geometry.

Two masks plus a couple of standalone toggles:

- **Render-type mask** (`Advanced.ToggleRenderType`) — per object-class
  visibility: simple / alpha / alpha-mask / fullbright / glow / materials / PBR
  / bump / tree / avatars / animesh / surface-patch / sky / water / ground /
  volume / grass / clouds / particles. Each is our renderer skipping that draw
  class.
- **Render-feature mask** (`Advanced.ToggleFeature`) — fog, foot shadows,
  flexible objects, dynamic textures, and the UI / selected / highlighted
  compositing overlays.
- **Standalone**: **Wireframe** (`Advanced.ToggleWireframe`, Ctrl+Shift+R) and
  **Hide Particles** (`hideparticles`) — the two most-reached-for.

Scope: a render-type / feature bitmask the draw systems consult before emitting
each class, the toggles wired to keybinds ([[viewer-input-action-map]]) and menu
items, and persistence of the non-default ones. These are cheap individually —
the value is having the whole set, because photographers compose with them.

The related *diagnostic* overlays (bounding boxes, octree, normals, shadow
frusta, the "info displays") are a separate concern — some already exist as our
debug camera / env toggles, and the rest belong with a dev-tools task, not here.

Reference (Firestorm, read-only): `menu_viewer.xml` Advanced → Rendering → Types
/ Features, `Advanced.ToggleRenderType` / `ToggleFeature` / `ToggleWireframe`.

Builds on: the existing draw systems (each already knows its object class).
