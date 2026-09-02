---
id: viewer-scene-dump
title: Emit the shared scene-dump JSON beside the frames
topic: viewer
status: done
origin: Firestorm cross-check harness plan (2026-09-01)
points: 5
refs: [viewer-render-pixel-oracle, viewer-screenshot-fixed-resolution]
---

Context: [context/testing.md](../context/testing.md).

Pixels say two viewers disagree. They do not say why, and the four
commonest causes look identical in an image: a prim in the wrong place, a
texture that resolved to a different asset id, a mesh stuck at a coarser
LOD, and a material that never arrived. Firestorm now writes a structured
dump next to its frames (`fstestscenedump.cpp`); this viewer must write
the same document or the comparison has nothing to compare.

Write `scene.json` into `--screenshot-dir` when the capture finishes, or
to `--scene-dump <path>`. The schema carries a `schema_version` — version
1 today — so a mismatch is an error rather than a confusing diff:

- `context` — viewer name, channel, version, grid, region name/id/handle,
  timestamp
- `camera` — origin and focus in both region and global coordinates, the
  three axes, FOV, aspect, near/far
- `environment` — day position, sun and moon direction, sun rotation, sky
  and water names
- `objects` — per object in the *agent's own region* (not neighbours, whose
  caching differs between viewers): id, local id, pcode, region position,
  rotation, scale, and per-face texture id, colour, scale/offset/rotation,
  bump, shiny, fullbright, glow, material ids; plus is-mesh, mesh id,
  is-sculpt, current LOD, flexible, light
- `avatars` — id, self flag, position, rotation, fully-loaded state
- `render` — draw distance, quality level, shadow and reflection-probe
  detail, max texture resolution, LOD factor

The units are the contract: region-local SL metres, Z-up, exactly as the
camera flags already take. Whatever this viewer stores internally, the
dump is in reference coordinates, so a diff never has to know which viewer
wrote which file.

Prefer sorting `objects` by id before writing. An unstable iteration order
turns every dump into a diff against itself and teaches whoever reads the
comparison to ignore it.

Refs [[viewer-render-pixel-oracle]] (the projection maths that already
turns a region position into a framing pixel) and
[[viewer-screenshot-fixed-resolution]].

Done (2026-09-02): `sl-viewer-world-view/src/scene_dump.rs`, written to
`<screenshot-dir>/scene.json` at the end of a capture or to `--scene-dump
<path>` (env `SL_VIEWER_SCENE_DUMP`). Every section and key is spelled as
`fstestscenedump.cpp` spells it, including the object class — which is now
`sl_proto::pcode::describe`, the reference's own `pCodeToString`, in the
pure crate beside the constants it spells.

**It reports what was drawn, not what arrived.** Positions and rotations
come back out of each entity's `GlobalTransform` rather than out of the
object update that produced it. A dump built from the received wire values
would agree with Firestorm's *by construction* — both viewers got the same
bytes — including in the case where our own transform maths puts the prim
somewhere else, which is the bug a cross-check exists to find. So the
inverse of the Second Life → Bevy basis change is applied on the way out,
and a paired test proves that inverse is the object layer's forward. Scale
is the one exception: no entity carries it, so the wire value is reported.
The write happens in `Last`, after transform propagation, or every pose
would be the previous frame's.

**Which ids two viewers can agree on** turned out to be the part worth
writing down, and the first live pair proved each case:

- **Viewer-side scene objects.** Firestorm reported 296 objects to our 21.
  The extra 275 are its own scenery — 256 `app-30` terrain patches plus
  sky, water and clouds — each with `local_id` 0 and a freshly minted id.
  This viewer does not model them as objects at all.
- **Control avatars.** An animesh rides a headless avatar with no grid
  identity, and Firestorm mints it a local UUID: two consecutive runs of
  the *same* viewer produced `36a77dc9…` and `ecce83a2…`. Ours reports one
  by the animesh **object** it rides, flagged `is_control_avatar`, so the
  pair is matched by flag and object and never by id.
- **Baked avatar textures.** A bake's id is minted by whoever baked it — a
  client bake on upload, a server bake per run — so two viewers can hold
  different ids for the same appearance. A baked slot's id is evidence a
  bake arrived, never that both viewers rendered the same one.

Three things the first diff found in the dump itself, each of which had
turned every object in the scene into a difference:

- `num_faces` was 64 everywhere. A texture entry states a default that
  applies to every face, so decoding one with the wire's 64-face maximum
  gives a six-sided box sixty-four faces (and a 599 KB document against
  Firestorm's 87 KB). It is now bounded by the object's own face count.
- Rotations differed by sign on prims where both viewers agreed. A
  quaternion and its negation are the same rotation; the dump now emits the
  non-negative-real form.
- `is_sculpt` was false for every mesh. The reference's `isSculpted()` is
  true for a mesh too (a mesh is a sculpt whose type says mesh), so a mesh
  reports both.
- The render settings this viewer stores as `u32` were read as `i32` and
  came out absent rather than equal.

What the pair still disagrees about, none of it schema noise, and all of it
for somebody else to chase:

- **The NPC's skull attachment** is at 27.06 m here and 26.20 m there, with
  a rotation against an identity one. Ours is the posed skull joint; the
  reference's `getPositionRegion` on a child is its parent-relative
  placement. Both may draw it in the same place — a dump-semantics
  difference to settle before it is read as a placement bug.
- **`lod` differs on five objects**, which the dump explains itself: the
  two viewers' `mesh_lod_boost` is 1.0 against 2.0 and their draw distance
  512 m against 128 m. That is the document doing its job.
- **`num_faces`** is the count this viewer *drew* and the count the
  reference *declares* (`getNumTEs`); they part on the sculpt sphere (1 vs
  6, its sculpt map having reached only one of them) and on the rigged mesh
  (0 vs 1).
- **`is_flexible`** is "declares itself flexible" here and "is being drawn
  flexible" there.

Ten unit tests, the load-bearing ones being the round trip through the
object layer's own placement (a swapped axis, a missing negation, a doubled
basis change and a forgotten region offset all produce a plausible dump)
and the reference key set, spelled out so a rename is a failing test rather
than a silent divergence in every object of every scene.
