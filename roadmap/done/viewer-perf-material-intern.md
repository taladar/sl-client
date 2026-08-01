---
id: viewer-perf-material-intern
title: Intern face materials by content for shared handles + draw batching
topic: viewer
status: done
origin: profiling design discussion (2026-07-31)
blocked_by: [viewer-perf-prim-tessellation-cache]
refs: [viewer-perf-prim-tessellation-cache, viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

The second half of the copy-paste-duplication recovery, split out from the
geometry cache ([[viewer-perf-prim-tessellation-cache]]) because the two have
very different hit rates and compose independently. The geometry half is
**done** (2026-07-31): identical prim / sculpt / mesh instances now share one
`Handle<Mesh>` per face through the `GeometryCache` resource
(`sl-client-bevy-viewer/src/geometry_cache.rs`), at a measured ~79% full-hit
rate on a login and ~86% under LOD churn. This item finishes the job for
whole-object duplicates: when the *materials* match too, Bevy's automatic
batching collapses the copies into instanced draws.

Today every face still builds its own material — `face_material(...)` /
`materials.add(...)` per face, no content keying — so even identically
textured faces get distinct `Handle<FaceMaterial>`s. Bevy batches draws only
when **both** the `Handle<Mesh>` *and* the material handle match, so distinct
material handles block batching even where the geometry cache already shared
the mesh.

## Approach (grounded in the implemented geometry cache)

- **Interception point:** `spawn_face_entity` (objects.rs) is now the single
  chokepoint every face path funnels through (prims, sculpts, meshes — cached
  and uncached), and it is exactly where `face_material` runs. Intern there,
  mirroring how the geometry paths intercept `meshes.add`.
- **Key:** a fingerprint of the material *inputs*: the decoded `TextureFace`
  content (texture id, tint bytes, repeats / offset / rotation → the
  `uv_transform`, glow, bump-shiny-fullbright byte, planar flag) plus the
  `TextureAlpha` mode the caller passes. The wire values are quantized, so
  re-encoding the face (or hashing the raw per-face wire bytes) gives an
  integer key with no float-hashing trouble — same trick as
  `PrimShapeParams`.
- **Lifetime:** copy the proven `GeometryCache` pattern verbatim — store weak
  `AssetId<FaceMaterial>`s, revive via `Assets::get_strong_handle`, prune
  dead entries on the same periodic sweep. No new eviction design needed.
- **Post-creation mutation is the real design work.** A face material is not
  immutable today:
  - `face_material` parks the material in `PrimTextures` until its texture
    decodes, then mutates it (texture handle, alpha upgrade). With a shared
    handle the same registration can arrive from several faces — the pending
    bookkeeping must tolerate duplicate registrations of one handle (the
    mutation itself is idempotent, same content → same result).
  - The later per-face material *rewriters* — PBR GLTF materials
    (`ObjectRenderMaterials` / materials.rs), legacy normal/specular
    (legacy_materials.rs), bump (bump.rs), per-face texture animation
    (`SlFaceExt` GPU params), media-on-a-prim — mutate a face's material
    in place. First iteration: **exclude** any face those systems touch
    (texture-animated, MoaP, PBR-/legacy-/bump-covered faces) from
    interning, exactly as the caveat below already planned; they keep
    per-instance materials. A later iteration can fold the PBR/legacy
    material id into the key instead.
- **Payoff check:** two same-shape same-texture copies (the rezzed duplicate
  rows on the local grid) should collapse to ~one instanced draw; verify via
  the F3 draws gauge / Tracy render spans, and confirm the store-vendor case
  (same shape, different texture) still shares the mesh while keeping
  separate draws.

## How it composes with the geometry cache

- **Shape matches, texture differs** (store vendor's wall of identical boxes,
  each a different product): geometry cache shares the mesh (one tessellation,
  one buffer); this intern does *not* fire on the differing face, so those draws
  stay separate — the geometry win is still fully realised. This is the common,
  high-value case for geometry-only dedup and why the two are split.
- **Shape and texture both match** (fence posts, a tree stand, a tiled floor):
  shared mesh *and* material → Bevy collapses them to ~one instanced draw.

So this item is the *bonus* that turns matched-texture duplication into a
draw-call win on top of the geometry cache's tessellation/GPU-memory win.

## Caveats

- `uv_transform` is material state, so faces differing only in repeats/offset
  get distinct materials and will not batch unless that placement is also
  identical (or later moved into per-instance data).
- Neither this nor the geometry cache reduces entity count or per-instance
  frustum culling — that is a separate lever (see
  [[viewer-perf-pipeline-specialization-stalls]]).
- Exclude materials that mutate per frame: per-face texture animation,
  media-on-a-prim, and any GPU-time-driven `SlFaceExt` effect cannot share a
  handle (see the mutation analysis above).
- Eviction/generation tracking mirrors the geometry cache (release on region
  teardown / teleport falls out of the weak-id + prune pattern).

## Done (2026-08-01)

Implemented as planned, mirroring the geometry cache:
`sl-client-bevy-viewer/src/material_cache.rs` holds the `MaterialCache`
resource (weak `AssetId`s, `get_strong_handle` revive, 30 s prune) keyed
by `MaterialKey` — the full decoded `TextureFace` content with the float
fields as exact `f32::to_bits` patterns (wire dequantization is
deterministic, so identical wire faces key equal with no float hashing)
plus the `TextureAlpha` mode. `intern_face_material` (textures.rs)
intercepts inside `spawn_face_entity`; on a hit while the texture is
still undecoded the shared handle is re-parked deduplicated-by-handle
and re-requested (priority boost + failed-decode retry both fall out).

Two things went beyond the plan:

- The exclusion inputs are object-level, but the deferred LOD / decode
  rebuilds run without the `Object` — so a `MaterialInternContext`
  (running texture animation, PBR-covered face indices, HUD membership)
  is computed per build and carried in `PendingPrim` / `PendingMesh` /
  `PendingSculpt`.
- Spawn-time exclusion alone is not safe: a late `llSetTextureAnim`, PBR
  data assigned to existing faces, HUD routing, and the edit floaters'
  live previews all mutate a face material **without** a texture-entry
  change (which would rebuild the faces and re-evaluate). Every interned
  face carries a `SharedFaceMaterial` marker, and a `PreUpdate`
  copy-on-write detach system gives any such face a private recompose
  before the Update-schedule mutators run; the marker filter keeps the
  steady-state sweep free. TE-carried mutators (legacy material id,
  bump, media) stay excluded at spawn and self-heal via the existing
  TE-change rebuild.

Measured via the new F3 `mat entries/hit/miss/excl` line:

- Local grid login: `entries 6, hit 652, miss 6, excl 22` — the
  duplicate-heavy sample content collapses onto six distinct materials.
- aditi (public sandbox, fully rezzed): `entries 2039, hit 18354,
  miss 2043, excl 2351` — ~90% of internable face spawns revived a
  shared material (~9× fewer material assets; only ~10% of faces are
  excluded as mutation-prone), so the matched-content copies now share
  both mesh and material handle, which is what Bevy's automatic
  batching keys on.
- No FPS regression attributable: three successive logins into the same
  sandbox scored 7–9 fps (interned, cache-cold first login), 17–22 fps
  (baseline, warmer cache), 35–58 fps (interned, warmest cache) — the
  ordering tracks texture-cache warmth / rez progress, and the
  interned build's fully-rezzed frames are the fastest of the set.
- Visual A/B (baseline vs interned, same aditi viewpoint): identical
  scene rendering, avatars (excluded path) included.
