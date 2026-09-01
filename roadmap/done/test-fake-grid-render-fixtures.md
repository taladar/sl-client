---
id: test-fake-grid-render-fixtures
title: Typed prim fixtures — textured, mesh, sculpt, PBR, light, particles, linksets
topic: test
status: done
origin: test-harness plan (2026-08-30)
points: 5
refs: [viewer-fake-grid-login-smoke]
blocked_by: [test-fake-grid-terrain-layerdata, test-shared-test-assets]
---

Context: [context/testing.md](../context/testing.md).

Both blockers cleared (2026-08-31): [[test-shared-test-assets]] shipped the
procedural pixels and [[test-fake-grid-terrain-layerdata]] the region's
ground, `RegionConfig::terrain` and the detail-texture assets.

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

Done (2026-09-01): `sl-fake-grid/src/fixtures/` — `prims.rs`
(`PrimFixture`, `FaceStyle`, `SculptKind`, `linkset`), `catalogue.rs`
(fifteen named prims in a row, `entries()` / `entry(name)`), and
`RegionFixture { world, assets, materials, media, environment,
terrain }` with `into_scenario()` / `into_region(base)`. The binary
grew `--catalogue`. `sl-test-assets` grew what the catalogue's prims
reference: `sculpt_sphere`, `mesh::unit_cube_mesh_asset` (the one mesh
*encoder* in the workspace) and `gltf_material_asset`; `sl-proto`
gained `attachment_state_from_point` and made `decode_extra_params`
public beside its existing `encode_extra_params`. Acceptance met by
`the_catalogue_reaches_the_client_field_for_field` in
`sl-fake-grid/tests/client_end_to_end.rs`, which compares the real
client's decode against the fixture's own blob, plus
`the_catalogue_assets_are_fetchable`. Every builder method is exercised
by the catalogue or a unit test — that is what keeps the list honest,
and why `.extended_mesh()` and `.sound()` were written and then dropped
rather than left unused.

Three deviations from the plan, all deliberate:

- `RegionFixture` has no `npcs` field: `NpcFixture` is
  [[test-fake-grid-npc-avatars]]'s to define, and a placeholder here
  would be dead forward-looking API.
- `FaceStyle` also carries `rotation`, `material` and `media`, without
  which the catalogue's legacy-material and MOAP prims could not be
  expressed. Two builder signatures needed a second argument the wire
  demands: `.mesh(key, faces)` (a mesh's face count is its submeshes',
  not derivable from the key) and `.pbr(face, material)`.
- The `sl-conformance` `Grid::Fake` branch the plan mentions does not
  exist yet — that is [[test-audit-fake-grid-conformance-grid]]'s. The
  catalogue is ready for it.

Also worth keeping: the flexi `ExtraParams` block quantizes its floats
to a byte each, so a fixture's typed `extra` and the client's decoded
`extra` agree only to the wire's resolution — assert against
`decode_extra_params(&fixture.extra_params)`, never against the typed
value. And `decode_texture_entry` fills as many faces as the *caller*
asks for, so a narrowed entry (a one-face mesh) shows up as a style
never reaching the wire, not as a shorter decode.
