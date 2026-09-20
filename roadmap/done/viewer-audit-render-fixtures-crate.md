---
id: viewer-audit-render-fixtures-crate
title: 3297 lines of test fixtures ship in the production scene crate
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
refs: [viewer-audit-binary-module-extraction]
---

Context: [context/viewer.md](../context/viewer.md).

Done (2026-09-20): the registry (4,082 lines by the time it moved) is the new
`sl-viewer-render-fixtures` crate, whose `lib.rs` *is* the former
`render_scene.rs` — a leaf that depends on the scene layer rather than a module
inside it. `sl-viewer-world-scene` is 30,322 lines down to 26,240, and the nine
crates that stand on it without ever building a scene (`sl-viewer-edit`,
`-environment`, `-map`, `-people`, `-places`, `-preferences`, `-search`,
`-ui-context-menus`, `-world-view`) no longer compile a fixture. In
`sl-client-bevy-viewer` it is a **dev**-dependency: the harness tiers were
already the only `#[cfg(test)]` users of the alias. The two consumers keep the
name they used — `pub(crate) use sl_viewer_render_fixtures as render_scene;` —
so no call site moved.

The move is verified the way [[viewer-audit-binary-module-extraction]] was: a
normalised token diff against `git show HEAD:...render_scene.rs` with the
intended rewrite (`crate::` → `sl_viewer_world_scene::`) applied to the old
side. Code and comments are character-identical afterwards apart from rustfmt
re-sorting the import block.

What the crate boundary cost, and it is the interesting half: 30 items the
fixtures reach down into had been `pub(crate)` — `resolve_sky` (and
`ResolvedSky`, with its fields), `water_params`, `water_specular_color`,
`WaterLighting`, `build_patch_mesh`, both `placeholder_image`s, the dome radii,
`drive_particles` and its `SystemParam` bundles. Widening them fired, in order,
five `private_interfaces` errors on the particle systems' signatures, 15
`must_use_candidate`, two `missing_debug_implementations` (one derive; one
`#[expect]`, since a `SystemParam` cannot derive it), and then 16
`private_intra_doc_links` from docs that had linked to a private neighbour
while nobody published them. `cargo machete` found the fourth: `bytes` and
`sl-avatar` left `sl-viewer-world-scene` with the fixtures, nothing else there
having used them.

The package closure is unchanged — 579 both sides, because `bytes` and
`sl-avatar` come back transitively through siblings. The win here is compile
volume and rebuild fan-out, not the graph, and a decoupling task that only
removes a direct edge should be measured before it is believed.

`sl-viewer-world-scene/src/render_scene.rs` is 3297 lines of **test fixtures** —
procedural prims, sculpts, meshes, skeletons, demo scenes — shipped as
`pub mod render_scene` with no `cfg` and no feature gate.

It is well-argued in its own module docs and genuinely valuable; it just belongs
in a `sl-viewer-render-fixtures` crate. Today it drags `sl-terrain`, `Bytes` and
a committed `.llm` into every consumer of the scene layer.

Same shape as the gallery modules in
[[viewer-audit-binary-module-extraction]] — harness code compiled into a library
22 crates depend on.
