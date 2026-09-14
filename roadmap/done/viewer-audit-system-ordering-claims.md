---
id: viewer-audit-system-ordering-claims
title: Update tuples claim a pipeline order the scheduler does not enforce
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

(The tuples have since moved from `lib.rs` to
`sl-client-bevy-viewer/src/viewer_plugins.rs`, where `ViewerWorldPlugins`
registers the world fold; the line numbers below are the ones the audit saw.)

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

One member of this family has since been found *live* rather than statically and
fixed: the world-reset purge and the object fold both read `SlEvent` with no
edge to the system that writes it, so they disagreed by a frame about the same
batch and a teleport arrived in an empty region
([[viewer-teleport-never-resets-the-world]]). The edge it needed did not exist
to be written — `sl_client_bevy`'s writer is private — so that fix added
`SlClientSystems::SessionDrained` as the name to order against. Any other reader
of `SlEvent` whose *effect* must land in a particular frame has the same
problem, and now has the same vocabulary for it.

## Resolution (2026-09-14)

Every tuple the audit named is now a named function returning a
`ScheduleConfigs<ScheduleSystem>`, chained, in `viewer_plugins.rs`:
`texture_store_pipeline`, `mesh_and_bake_pipeline`, `face_material_pipeline`
and `environment_asset_pipeline`. The region-environment (EEP) fold got the
same treatment: it was not on the audit's list but is the same defect in the
same `add_systems` call, and it carries the most explicit prose of all —
"Before the ingest", "After the ingest", "Last of the four", "Last, so both see
the frame's final environment" — none of which the scheduler was enforcing.

**Functions rather than inline tuples, because that is what makes the order
testable.** `ScheduleConfigs` is a value, so a test can put one into a bare
`Schedule`, initialize it against an empty `World`, and read back what the
scheduler will enforce. A comment claiming an order cannot be tested; this can.

**Three of the edges cross tuple boundaries**, and those were the claims most
worth keeping:

- `face_material_pipeline` is `.after(poll_textures)` — the texture tuple's
  comment always said the poll runs "before the consumers that apply them", and
  the consumers are in a different tuple;
- it is also `.after(WorldPhase::ObjectsUpdated)` and
  `.after(apply_object_meshes)` — its own comment said it "runs after the
  face-spawning systems so a face's PBR material is seen";
- `mesh_and_bake_pipeline` is `.before(apply_object_meshes)`, the consumer of
  the `MeshDecoded` messages `poll_meshes` writes.

**No sync points were bought with this.** Bevy's `.chain()` inserts an
`ApplyDeferred` on an edge whose source has deferred parameters; not one of the
~45 systems involved takes `Commands`, so the chains are pure ordering. They
also cost little parallelism: the stages of each pipeline contend for the same
`ResMut` (the texture / mesh / material managers) and could never have run
together anyway.

Unit-verified, six tests in `viewer_plugins.rs`. They assert the ordering
**edges**, not the order the systems are stored in — that distinction is the
whole point. Bevy's topological sort of an unconstrained graph still produces
*some* order, and it was measured to produce the right one for two of the three
cross-tuple claims by luck alone, so a test reading the stored order would have
passed against the bug. The tests instead flatten `ScheduleGraph::dependency()`
through the hierarchy (a set stands for every system beneath it) and ask
whether the edges *reach* from each stage to the next. Each test was confirmed
to fail with its `.chain()` or `.after()` removed.

The one-time legacy-specular edit crash on aditi remains unreproduced, so this
is not claimed as its fix — only as the removal of one of the candidates for it.
