---
id: viewer-mesh-model-upload
title: Mesh / model importer & upload
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework, viewer-prim-texture-editing]
---

Context: [context/viewer.md](../context/viewer.md).

The big creator cluster: import a model, drive the model-preview floater with
four discrete geometry **LODs**, a **physics** shape, **skin / joint** binding
for rigged mesh, material assignment and a **land-impact / L$ estimate**, then
serialise it into the LLMesh asset and upload. Also Firestorm's **local mesh**
(preview a mesh from disk in-world before uploading).

We already have the LLMesh **decoder** (`sl-mesh`: LODs, `skin`,
`physics_convex`, `physics_mesh`). This task is the inverse — the encoder — plus
the creator-tool math. The upload cap itself is generic
(`NewFileAgentInventory`, done as `protocol-23`), so nothing mesh-specific
exists on the send side yet.

## Import: glTF only, pure-Rust (surveyed 2026-07)

**Required importer: the pure-Rust `gltf` crate** (MIT/Apache). It covers
everything the SL rig needs — multiple primitives (submeshes), material refs,
morph targets, GLB, and full skinning (`Skin::joints` /
`inverse_bind_matrices`, per-primitive `read_joints` / `read_weights`, the node
hierarchy). What is left is *our* math, not a crate gap: glTF→SL coordinate
conversion (glTF is Y-up right-handed, SL is Z-up), mapping glTF joint node
names onto the fixed SL skeleton, synthesising SL's bind-shape matrix from the
node transforms, and clamping/renormalising to ≤4 influences/vertex.

**COLLADA is deliberately out of the required set.** Blender has dropped its
`.dae` exporter and SL content authoring has moved to glTF — Firestorm's own
glTF importer **converts to the identical LLMesh format** it uses for COLLADA,
so glTF-first costs nothing on the encode side. Do **not** pull in
`russimp`/assimp for `.dae`: it is a heavy C++ dependency, and it was archived
and unmaintained in late 2025 anyway. If `.dae` is ever wanted, add it later as
a separate optional importer over a pure-Rust crate (`dae-parser` reads *and*
writes) — never as a reason to take a C++ toolchain.

## LOD, physics, cost — and a history lesson

**This feature is where SL's own "avoid platform-specific C++" scars are.** Mesh
*upload* was disabled on the Linux (and 64-bit) viewer for years because its LOD
generator, **GLOD**, was not redistributable / not buildable there. Linden Lab
fixed it by replacing GLOD with **meshoptimizer**. Our design starts on the far
side of that fix, and stays pure-Rust exactly where GLOD and Havok were the
liabilities.

- **LOD decimation → the `meshopt` crate**, and this is a place we can *beat*
  the reference rather than merely match it. It is the one C++ dependency in the
  whole importer (verified not already in our tree — Bevy's meshlet feature
  would link it, but the viewer does not enable that), and it is the acceptable
  kind: a small, self-contained, dependency-free, MIT-licensed C++ TU compiled
  via `cc` (no cmake, no system libs). `meshopt_simplify` is itself a proper QEM
  (Garland–Heckbert) edge-collapse — *not* a speed hack; the topology-ignoring
  `simplifySloppy` is a separate call for the farthest LOD only. It also does
  the geometry prep the encoder wants (`generate_vertex_remap` weld/index,
  `optimize_vertex_cache`, `optimize_vertex_fetch`).

  The known knock on meshopt — Firestorm keeps GLOD as a "reliable" toggle
  because meshopt is worse ~2/3 of the time — is
  **specifically UV-seam / attribute damage**, and the reference viewers
  *under-use* `simplifyWithAttributes` with UV weighting and seam locking. Using
  that path properly is how we close most of the GLOD gap while staying in one
  algorithm. (The "avoid FFI to dodge crashes" argument does **not** apply here:
  the SL Linux glTF-upload crashes are in the import / validation path, not the
  decimator, and meshoptimizer is among the most battle-tested C libraries in
  graphics.)

  Pure-Rust alternatives were surveyed and are watch-list, not ready:
  **`baby_shark`** (0.3.12, actively maintained, real boundary-aware QEM, MIT)
  is the healthiest, but its collapse cost is
  **geometry-only — no UV / attribute term**, so it would regress the exact axis
  SL creators care about; keep it as a candidate for an optional "reliable"
  second pass on *untextured* meshes only, and a real contender the day it grows
  attribute-aware quadrics. The pure-Rust `meshopt-rs` port's `simplify` is
  unreleased and abandoned since 2022.

- **Physics "Analyze" → `parry3d`'s pure-Rust V-HACD, which we already ship**
  (via avian3d). `VHACD::decompose` + `compute_convex_hulls` returns one hull
  per part — exactly the `physics_convex` `HullList` shape — and parry's own
  `convex_hull` covers the single bounding hull. So the heaviest-sounding piece
  needs no new dependency and no C++. (The stock LL viewer used Havok here;
  Firestorm swapped in an open V-HACD, which is what we mirror.) Pure-Rust CoACD
  (`CoACD-rs`, WIP) is a future higher-quality tier if wanted — still no C++.
  Match Firestorm's decomposition knobs: max hulls (default 8, ≤256), vertices
  per hull (default 32, ≤256), error tolerance, voxel resolution.

- **Streaming cost / land impact → ours, from the viewer's own formula.** It is
  reproducible client-side: est-triangles-per-LOD from section byte size
  (`MeshBytesPerTriangle = 16`, `MeshMetaDataDiscount = 384`, a floor), radius-
  weighted over LOD-switch annuli (`radius/0.03`, `/0.06`, `/0.24`; 512 m view
  cap; ~102944 m² region area), times `15000 / MeshTriangleBudget`. Match the
  viewer's `LLMeshCostData` (in `secondlife/viewer`, not the wiki) for exact
  numbers. **But the L$ upload *fee* is server-side** — the viewer only displays
  it — so a fee shown before upload needs the step-1 fee round-trip (below).

## The encoder (ours — inverse of `sl-mesh`)

Reference `sl-llsd` (binary LLSD) + `flate2` (zlib). The format, spec-exact from
`llmodel.cpp`:

- **Header** is uncompressed binary LLSD: a map of section →
  `{offset, size}`, offsets relative to the end of the header, in write order
  `skin`, `physics_convex`, then the LOD/`physics_mesh` blocks. Section names:
  `lowest_lod` / `low_lod` / `medium_lod` / `high_lod` (the last required, each
  requiring the next-higher), `physics_mesh`, `physics_convex`, `skin`.
- **Each block** is binary LLSD then **zlib deflate at level 9**.
- **Quantization**: `Position` = 3×u16 across a per-model `PositionDomain`
  (Min/Max, written into every face); `Normal` = u16 over the fixed [-1,1];
  `TexCoord0` = 2×u16 across a **per-face** `TexCoord0Domain`; `TriangleList` =
  u16 indices. `physics_convex` = a `HullList` (u8 point-count per hull) plus
  u16-quantized `Positions` over its own domain, and `BoundingVerts` for the
  base hull. Tangents are not transported.
- **Weights** (skin submesh): per vertex, up to four `(u8 joint, u16 weight)`
  pairs, `0xFF` terminating a list shorter than 4. `skin` block:
  `joint_names`, flattened 16-float `bind_shape_matrix`, per-joint
  `inverse_bind_matrix`, optional `alt_inverse_bind_matrix` /
  `lock_scale_if_joint_position` / `pelvis_offset` for joint overrides.
- **Limits to enforce**: ≤8 faces per model, u16 indices, a lower LOD may not
  have more vertices than the LOD above it, ≤110 joints, joint index ≤254,
  ≤256 hulls and ≤256 points per hull.

## Upload sequence

Two POSTs of LLSD to the `NewFileAgentInventory` cap. **Step 1** posts the model
without textures and gets back
`{state: "upload", uploader: <url>, upload_price, data: {…costs…}}` — this is
where the fee and land-impact come from, so it doubles as the confirmation the
user approves. **Step 2** posts `asset_resources` (now with J2C textures) to the
returned uploader URL: `mesh_list[]` (the raw binary LLMesh assets),
`texture_list[]`, and `instance_list[]` (per-instance transform,
`physics_shape_type`, mesh index, per-face material). Good candidate to split
the encoder out as a pure `sl-mesh` `encode` feature (mirroring the existing
decode) beneath the viewer-side floater and preview.

Reference (Firestorm, read-only): `llmodel.cpp` (`writeModel` — the whole
encoder), `llmodelpreview.cpp` (`genMeshOptimizerLODs`, the LOD targets),
`llmeshrepository.cpp` (`LLMeshCostData`, `LLMeshUploadThread`, the two-step
upload), `llconvexdecompositionvhacd.cpp` (the open V-HACD Firestorm uses in
place of Havok), `llfloatermodelpreview`, `fslocalmeshimportgltf` /
`vjlocalmesh*` (local mesh).

Builds on: `sl-mesh` (encode = inverse of the existing decode), `sl-llsd`,
`flate2`, `parry3d` (already in the physics stack), and the `protocol-23` upload
caps.

Deps: [[viewer-ui-framework]], [[viewer-prim-texture-editing]] (material /
texture assignment overlap).
