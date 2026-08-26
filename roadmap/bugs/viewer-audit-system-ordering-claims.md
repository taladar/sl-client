---
id: viewer-audit-system-ordering-claims
title: Update tuples claim a pipeline order the scheduler does not enforce
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

Several system tuples in `sl-client-bevy-viewer/src/lib.rs` are documented as
pipelines and scheduled as plain tuples, so Bevy runs them in arbitrary order
and each stage's output is visible a nondeterministic 1-6 frames later:

- `:1957` — `(update_texture_caps, sync_texture_blacklist, poll_textures,
  serve_texture_boosts)` under a comment saying "keep the cap current, *then*
  poll finished fetches *before* the consumers";
- `:1969` — the mesh / wearable / bake tuple, "keep the cap current ... then
  assemble each bake region's layer list";
- `:2280` — `(update_environment_asset_caps, poll_environment_assets)`;
- `:2045-2082` — the whole PBR and legacy-material chain
  (`register_pbr_materials` -> `poll_materials` -> `apply_material_overrides` ->
  `apply_pbr_textures`, and the six-stage legacy chain
  `register_legacy_materials` -> ... -> `apply_legacy_specular_maps`), described
  as a pipeline in the comments and scheduled with no edges at all. Only the
  inner texture tuple is chained.

Compare `:2038-2044` and `:2002-2003`, which **do** `.chain()`.

The legacy-material one is a live candidate for the one-time,
non-reproduced legacy-specular edit crash seen on aditi.

Fix: add `.chain()` or explicit `.after()` edges so the schedule matches the
comments — or correct the comments where the order genuinely does not matter.
