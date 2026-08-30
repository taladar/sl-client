---
id: test-fake-grid-render-fixtures
title: Typed prim fixtures — textured, mesh, sculpt, PBR, light, particles, linksets
topic: test
status: blocked
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-fake-grid-login-smoke]
blocked_by: [test-fake-grid-terrain-layerdata, test-shared-test-assets]
---

Context: [context/testing.md](../context/testing.md).

`full_update_block` emits only the raw byte fields of an `Object`, so a
fixture must encode `texture_entry`, `extra_params`, `particle_system` and
`texture_anim` itself, and today's only builder is an untextured
`box_prim`. Add `sl-fake-grid/src/fixtures/prims.rs`:

`PrimFixture::boxed(..).shape(..).textured(key).face(i, FaceStyle {
texture, color, alpha, glow, fullbright, shiny, bump, repeats, offset })
.mesh(key).sculpt(key, kind).pbr(material).light(..).flexi(..)
.particles(..).texture_anim(..).hover_text(..).media_url(..)
.reflection_probe(..).child_of(parent, offset, rot)
.attached_to(avatar, point, item).build()`, `linkset(root, children)`,
and `RegionFixture { world, assets, materials, media, environment,
terrain, npcs }::into_scenario()` whose setup hook installs region
materials, object media and the environment. A named catalogue
(`fixtures::catalogue()`) is what the viewer harness, the conformance
`Grid::Fake` branch and the Firestorm binary all load.

Acceptance: the tokio end-to-end suite decodes typed `extra`,
`texture_entry`, particle and texture-animation fields equal to what was
seeded, for every builder method.
