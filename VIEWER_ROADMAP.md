# Visual viewer road map

A staged plan for a minimum-viable **Bevy visual viewer** on top of the existing
`sl-client` stack: log in via the current `credentials.toml` mechanism and
render a region — terrain, prims (full Linden path/profile tessellation),
meshes, and sculpt-texture prims — with diffuse textures (no advanced
materials), sphere placeholders for avatars, an on-screen chat overlay, a debug
fly-camera, and a single quit key.

Everything the protocol side needs already exists: the sans-IO `Session`
(sl-proto), the `sl-client-bevy::SlClientPlugin` ECS driver, and the asset
pipelines (`sl-texture` J2C→RGBA8, `sl-mesh` LLMesh→geometry). What is
missing is a **rendering** application — nothing today opens a window or draws
the region.

This is a large effort, so work it **top-to-bottom, one phase (or one point) per
session**: implement, build, run/test, commit the result on the current branch,
then tick the box here. Add sub-points as you discover them.

## New artifacts

- **`sl-prim`** — new pure crate (no Bevy, no I/O): Linden prim tessellation
  (path × profile sweep → geometry), mirroring `sl-mesh` / `sl-texture`.
- **`sl-terrain`** — new pure crate (no Bevy, no I/O): terrain texture-splat
  blend-weight math (elevation bilinear interpolation + Perlin transition band →
  per-point four-texture weight), added in P2.2, mirroring `sl-prim` /
  `sl-mesh`.
- **`sl-sculpt`** — new pure crate: sculpt-texture (RGB sculpt-map) → geometry,
  reusing `sl-prim`'s `PrimMesh` / `PrimFace` output type.
- **`sl-avatar`** — new pure crate (no Bevy; I/O-free, parses from bytes like
  `sl-mesh`): avatar skeleton (`avatar_skeleton.xml`), legacy base-body `.llm`
  mesh decode, the visual-param / morph-target / skeletal-scale / driver system
  (`avatar_lad.xml`), and generic matrix-palette skinning math shared by the
  base body and rigged mesh (added in Phase 12).
- **`sl-anim`** — new pure crate (no Bevy; I/O-free): Linden keyframe-motion
  (`.anim`) decode → per-joint keyframe tracks + priority / ease / loop /
  constraint metadata (added in Phase 18).
- **`sl-bake`** — new pure crate (no Bevy; I/O-free, depends on `sl-texture`
  with `default-features = false` for just `DecodedImage`, like `sl-sculpt`):
  **client-side** avatar bake — composite the wearable layer images + layer
  params (order, tint, alpha mask, tex-gen) into a baked RGBA per bake region.
  This is what OpenSim (legacy `UploadBakedTexture` client-bake) and any grid
  that doesn't server-bake require; the SL "Sunshine" server bake is the other
  path (added in Phase 15).
- **`sl-client-bevy`** — a small addition: a `to_bevy_prim_mesh` conversion +
  re-exports, mirroring the existing `to_bevy_mesh` / `to_bevy_image`; later
  (Phases 13–18) it also gains skeleton-instance + `SkinnedMesh` conversions
  and an animation driver, mirroring the existing `to_bevy_*` additions.
- **`sl-client-bevy-viewer`** — new binary crate: the windowed viewer app.

## Scope reminders

- Commit on the current branch only — never auto-create a feature branch.
- Keep the geometry crates (`sl-prim`, `sl-sculpt`) **Bevy-free**, mirroring
  `sl-mesh` / `sl-texture`; the `to_bevy_*` conversion lives in
  `sl-client-bevy`.
- Never push viewer/geometry types into the shared `sl-types` crate.
- The viewer consumes only `SlEvent` / `SlCommand` (it never calls `Session`
  accessors directly — the plugin encapsulates the session). It builds its own
  ECS scene mirror from the event stream.
- Keep `sl-client-tokio` and `sl-client-bevy` at feature parity where a change
  touches shared re-exports.
- Workspace restriction lints apply everywhere: no `unwrap` / `expect` / `panic`
  / `as` casts / `[]` indexing; docs on every item, including private ones. The
  tessellation math (trig + array access) is where this bites hardest — build
  accessor helpers over raw indexing and keep arithmetic in `f32`.
- `cargo fmt --all` and `rumdl` (on touched `.md`) before every commit; the
  `ggh` hook rejects on fmt / MD013 and re-runs full clippy. Never
  `cargo clean --doc`.
- Wrap this file at 80 columns.

## Legend & conventions

- Status: `[ ]` todo, `[x]` done. Tick a point only when it builds, is
  clippy-clean under the workspace restriction lints, and its tests (pure
  crates) or live check (viewer phases) pass.
- Pure-crate phases (`sl-prim`, `sl-sculpt`) verify with
  `cargo test -p <crate>`; viewer phases verify with a live run against the
  local **OpenSim** grid.
- First target grid is local OpenSim (terrain + prims + a provisioned mesh, no
  MFA). Aditi / real SL work through the same `credentials.toml` path later.

## Key facts (for implementers)

- Driver pattern: `sl-client-bevy/examples/survey_probe.rs` — read
  `MessageReader<SlEvent>`, emit `MessageWriter<SlCommand>`. Scene input events:
  `ObjectAdded` / `ObjectUpdated` / `ObjectRemoved`, `TerrainPatch`,
  `AvatarAppearance`, `CoarseLocationUpdate`, `ChatReceived`, `TextureReceived`.
- `sl-client-bevy` is headless today (`bevy_asset` / `bevy_image` / `bevy_mesh`
  only). The viewer adds `DefaultPlugins` (window + `bevy_render` / `bevy_pbr` /
  `bevy_ui` / `bevy_text` / `bevy_winit`). Bevy is `0.19.0`.
- Reuse: `to_bevy_image` (`textures.rs`), `to_bevy_mesh` / `to_bevy_meshes`
  (`meshes.rs`); fetchers `BevyTextureFetcher` / `BevyMeshFetcher` /
  `BevyAssetFetcher`; login via `sl_repl::auth::Credentials` (`sl-repl/
  src/auth.rs`) → `LoginParams` / `LoginRequest`.
- Object classification: avatar = `pcode == 47`; mesh = `extra.sculpt ==
  SculptOrMeshKey::Mesh(_)`; sculpt-texture = `SculptOrMeshKey::Sculpt(key)`;
  otherwise a tessellated prim. Shape params: `PrimShapeParams`
  (`sl-proto/src/types/object.rs`), with a float `PrimShape` companion.
- Coordinate systems: SL is right-handed **Z-up**, Bevy is **Y-up**. Geometry
  crates stay in SL space; a single `sl_to_bevy` conversion is applied only at
  the entity `Transform` / camera boundary in the viewer.
- Tessellation reference (read-only, reimplement idiomatically — do not copy):
  Firestorm `indra/llmath/llvolume.cpp` — `LLProfile::generate` / `genNGon`,
  `LLPath::generate` / `genNGon`, `LLVolume::generate`,
  `LLVolumeFace::createSide` / `createCap`, `LoDTriangleCounts`; sculpts:
  `LLVolume::sculpt`, `sculptGenerateMapVertices`.

---

## Phase 0 — Scaffold the three new crates

- [x] **P0.1. Create the crate skeletons.** Add `sl-prim/`, `sl-sculpt/`,
  `sl-client-bevy-viewer/`, each with a `Cargo.toml` (`edition = "2024"`,
  `rust-version = "1.94.0"`, `publish = false`, `[lints] workspace = true`), a
  `CHANGELOG.md` (`# Changelog` / `## 0.1.0` / `Initial Release`), and a
  `cliff.toml` copied from `sl-mesh/cliff.toml` with the crate's own
  `tag_pattern` (`^sl_prim_[0-9.]*$`, `^sl_sculpt_[0-9.]*$`,
  `^sl_client_bevy_viewer_[0-9.]*$`) and matching version trim.
- [x] **P0.2. Register the members.** Add `"sl-prim"`, `"sl-sculpt"`, and
  `"sl-client-bevy-viewer"` to the root `Cargo.toml` `members` array.
- [x] **P0.3. Green build.** Stub `lib.rs` / `main.rs` so
  `cargo build --workspace` succeeds.

## Phase 1 — Viewer shell (window, login, camera, quit)

- [x] **P1.1. Login from credentials.** `clap` args `--credentials <path>` /
  `--avatar <name>`; load via `Credentials::load().select()`; resolve the grid
  from `login_uri` / `grid` (default local `http://127.0.0.1:9000/`); acquire
  MFA via `Avatar::acquire_mfa()` + `LoginRequest::with_mfa` when configured.
  Build `LoginParams` and add `SlClientPlugin` (mirror `survey_probe.rs`).
- [x] **P1.2. Windowed app.** `App` with `DefaultPlugins`; spawn a `Camera3d`
  and a directional light. Milestone: a window opens and the session logs in
  (tracing shows the circuit + region handshake).
- [x] **P1.3. Debug fly-camera.** WASD translate, Shift = fast, mouse-look on a
  captured cursor; camera starts at the agent login position (via `sl_to_bevy`).
- [x] **P1.4. Quit + draw distance.** `Esc` / `Q` sends
  `Command::Logout` then `AppExit::Success`; also exit on `LoggedOut` /
  `Disconnected`. On `RegionHandshakeComplete` send
  `Command::SetDrawDistance(Distance::new(128.0))` so the sim streams content.

## Phase 2 — Terrain

- [x] **P2.1. Heightfield patches.** On `TerrainPatch`, build a mesh for the
  patch (grid of cells at `values[..]`, computed normals, whole-region UVs)
  placed at its `patch_x * size, patch_y * size` origin (`sl_to_bevy`); keep a
  `HashMap<(patch_x, patch_y), Entity>` and replace on update. One flat
  `StandardMaterial` (no splatting). Verify terrain renders on OpenSim.
- [x] **P2.2. Height-blended texture splatting.** Replace the flat ground
  material with the real Second Life terrain shading: the region's four
  `TERRAIN_TEXTURE_*` UUIDs and per-corner low/high elevation ranges (from the
  `RegionHandshake` / region info), blended by elevation with a Perlin-noise
  transition band (Firestorm `llvosurfacepatch` / terrain shaders,
  `llvlcomposition` for the CPU reference). Factor the Bevy-free blend-weight
  math into a new **`sl-terrain`** crate (mirroring `sl-prim` / `sl-mesh`), with
  the `StandardMaterial`/custom material living in `sl-client-bevy`; fetch the
  four textures through the existing texture pipeline. **Done (GPU path):**
  `sl-terrain` emits a per-vertex four-component blend weight; a custom
  `TerrainMaterial` (`AsBindGroup`, four detail-texture bindings) +
  `terrain.wgsl` in `sl-client-bevy` (behind a new `bevy_pbr` feature the viewer
  enables) blends the four live textures on the GPU with the interpolated
  weights + simple sun lighting. `RegionIdentity` gained a
  `terrain: RegionTerrainComposition` field.
- [x] **P2.3. Seamless patches + multi-region terrain.** Two fixes discovered
  when rendering live: (a) each patch mesh now spans its full 16 m edge —
  `(size+1)²` vertices sampling the far edge from the north/east/NE neighbour
  patches (Firestorm `LLSurfacePatch` stitching) — closing the 1 m gaps that
  made P2.1/P2.2 terrain look fragmented; (b) terrain now streams and renders
  across the agent's region **and** its neighbour child circuits: patches are
  keyed by `(region_handle, patch_x, patch_y)`, each region has its own
  composition + splat material, and patches are placed at a global offset from a
  moving scene origin that follows the root region (recenter shifts the
  fly-camera by the same delta so `f32` precision holds far from the login
  region while the world stays continuous across border crossings). The draw
  distance was raised to 512 m so the sim announces neighbours. Required one
  `sl-proto` fix: a neighbour's `RegionHandshake` on a child circuit now also
  emits `RegionInfoHandshake` (previously dropped), so neighbour terrain gets
  its own textures rather than the placeholder.

## Phase 3 — `sl-prim` (pure Linden prim tessellation)

- [x] **P3.1. Types & LOD.** `PrimLod` newtype + a detail→step-count map
  (details `{1.0, 1.5, 2.5, 4.0}`, profile sides `6 * detail`); output
  `PrimMesh { faces: Vec<PrimFace> }`, `PrimFace { positions, normals, uvs,
  indices, face_id }` (mirror `sl_mesh::DecodedMesh` / `Submesh`). Confirm or
  derive the float `PrimShape` input from `PrimShapeParams`.
- [x] **P3.2. Profile ring.** `profile.rs`: 2D profile (square / circle /
  half-circle / triangles) via `genNGon`, with profile begin/end cut and hollow
  (`addHole`) plus the semantic face-index ranges. A `Profile` of
  `ProfilePoint`s (2D position + sweep-parameter `u`) and `ProfileFace` ranges
  (`index`/`count`/`scale_u`/`cap`/`flat` + a `ProfileFaceId` `LL_FACE_*` bit
  flag), built by a private `Builder` mirroring `LLProfile::generate` /
  `genNGon` / `addHole` / `addCap` (per-edge `split`, path caps, open-ring
  profile edges, sphere-close special case).
- [x] **P3.3. Extrusion path.** `path.rs`: line / circle / circle2 paths
  applying twist, scale, shear, taper, radius-offset, skew, revolutions, and
  path begin/end cut.
- [x] **P3.4. Sweep & faces.** `volume.rs`: sweep the profile along the path and
  assemble per-face vertices / normals / UVs / indices (`createSide` /
  `createCap`, fan-triangulated caps), carrying the Linden `face_id`. A public
  `tessellate` builds the swept vertex grid (each profile point placed into
  each path frame), then emits one `PrimFace` per semantic profile face — the
  i-th face becoming Linden face index `i`. Sides are a `count × path.total`
  grid strip (grid positions, sweep-parameter/`tex_t` UVs, two-triangles-per-
  cell indices, accumulated-then-normalized normals with the reference viewer's
  closed-seam / pole normal wrapping); caps are a centre-vertex triangle fan
  with planar UVs and one flat normal. Two documented MVP simplifications in the
  road map's "fan-triangulated caps" scope: hollow caps are a filled centre fan
  (no annulus triangulation), and a hollow inner wall is a single smoothed strip
  (no flat-column doubling).
- [x] **P3.5. Shape tests.** Unit tests asserting non-degenerate counts and
  correct face counts: box (6), cylinder, sphere, torus, hollow box (+ inner
  face), cut prim (+ cut-edge faces). Deterministic-fixture style, as in the
  `sl-mesh` tests. `cargo test -p sl-prim`.

## Phase 4 — `sl-client-bevy` conversion

- [x] **P4.1. `to_bevy_prim_mesh`.** Add `to_bevy_prim_mesh(&PrimFace) -> Mesh`
  and `to_bevy_prim_meshes(&PrimMesh) -> Vec<Mesh>` (TriangleList; POSITION +
  optional NORMAL + UV_0 + `Indices::U32`), an analogue of `to_bevy_mesh`. Add
  the `sl-prim` dependency; re-export the conversion and the `sl_prim` types the
  viewer needs (`PrimShape` aliased `PrimShapeFloat` so it does not collide with
  `sl_proto`'s quantized rez-params `PrimShape`). `sl-prim` is a pure geometry
  crate with no store/fetcher, so — unlike `sl-mesh` / `sl-texture` — it has no
  `sl-client-tokio` runtime counterpart and this stays a `sl-client-bevy`-only
  change. The CHANGELOG is `git-cliff`-generated from commits, so no manual
  entry was added.

## Phase 5 — Prim rendering in the viewer

- [x] **P5.1. Object lifecycle.** New `objects.rs` module: an `ObjectState`
  resource keying every in-world object by `ScopedObjectId`, folded from the
  session event stream by the `update_objects` system. On
  `ObjectAdded` / `ObjectUpdated` it spawns/updates an entity tagged with a
  `SceneObject { scoped_id, category }` marker classifying it (avatar / plain
  prim / sculpt / mesh / other, from `pcode` + the sculpt/mesh `ExtraParams`);
  on `ObjectRemoved` it despawns the entity (Bevy's hierarchy takes its parented
  children) and drops it plus any tracked descendants from the map. A **root**
  object's `Transform` is a world transform (`sl_to_bevy_vec` position +
  `sl_to_bevy_object_rotation` — the basis change composed with the object's own
  orientation); a **child** gets a *local* transform kept in pure Second Life
  space (`sl_rotation_to_quat`), parented via `ChildOf` to its root so the root
  carries the single basis change for the whole linkset. A child that arrives
  before its root is held parentless and adopted once the root appears
  (`adopt_pending_children`); a runtime relink/unlink re-parents on update
  (`reconcile_parent`). A `ShapeFingerprint` (pcode, the quantized
  `PrimShapeParams`, and the sculpt/mesh key) is compared per update so a
  motion-only update never flags a re-tessellation (consumed in P5.2). Two
  rotation helpers were added to
  `coords.rs` (`sl_rotation_to_quat`, `sl_to_bevy_object_rotation`). No geometry
  is spawned yet — the entities carry only a `Transform` + marker, which P5.2 /
  P7 / P9 / P10 hang meshes on. This stays a `sl-client-bevy-viewer`-only change
  (no region-origin offset yet: objects sit in the root region's frame, aligned
  with the home terrain and camera, as P2 does).
- [x] **P5.2. Tessellated prims.** For a plain prim, tessellate with
  `sl_prim` at a fixed High LOD and spawn one child entity per `PrimFace` (so
  each face can carry its own material). Verify box / cylinder / sphere / torus
  render correctly positioned on OpenSim. **Done:** `build_prim_faces`
  tessellates a
  `Prim`-category object (`PrimShapeFloat::from_params` → `tessellate(_,
  PrimLod::High)`) and spawns one `Mesh3d` child per non-empty face
  (`to_bevy_prim_mesh`), parented via `ChildOf` to the object entity so the
  object's `Transform` carries the object scale / rotation / position and the
  single SL→Bevy basis change; a shape-fingerprint change despawns and rebuilds
  the face children (`despawn_prim_faces`), a motion-only update never
  re-tessellates. Each face carries a `PrimFaceEntity { face_id }` marker for
  the Phase 6 per-face texturing pass to key off. Until Phase 6 every face
  renders with one shared neutral placeholder `StandardMaterial` (double-sided /
  culling off, so a face shows regardless of winding). Two live findings: (a)
  the object entity now also carries `Visibility::default()` — the `Mesh3d` face
  children add `Visibility`, and Bevy's visibility propagation warns (B0004) if
  the parent has none; (b) the hollow-cap MVP simplification from P3.4 is
  visible on OpenSim — a hollow prim's cap fills its hole, so a hollow prim
  reads as a solid-capped tube. Verified live on OpenSim (4 prims + 1 mesh + 1
  avatar streamed and tessellated; prims render untextured — texturing is P6).

## Phase 6 — Texturing (diffuse only)

- [x] **P6.1. Per-face diffuse.** Decode each face's
  `TextureEntry.faces[face_id]` (`decode_texture_entry`); request the texture,
  convert the decoded RGBA8 with `to_bevy_image`, and build
  `StandardMaterial { base_color_texture, base_color = face tint }`. Dedupe
  with `HashMap<TextureKey, Handle<Image>>`; faces whose texture has not
  arrived use a flat colour from `face.color`. No normal / specular / PBR /
  glow / bump. **Done — via the shared `TextureStore`, not inline decode.** On
  user direction the viewer drives the LOD-aware `sl_texture::TextureStore`
  (the same fetch / off-thread-decode / Firestorm-disk-cache / weak-ref-dedupe
  pipeline the headless client uses) rather than decoding J2C on the render
  thread. A new `textures.rs` module owns a `TextureManager` resource (store
  over a `BevyTextureFetcher` whose `GetTexture` cap URL is refreshed from
  `SlCapabilities`); each texture is fetched on a background `IoTaskPool` task
  (blocking HTTP off-thread, decode on the store's own rayon pool), and
  `poll_textures` folds a finished decode into a shared cache and announces it
  with a `TextureDecoded` message. `build_prim_faces` decodes the object's
  `TextureEntry`, builds one `StandardMaterial` per face (tint now, texture
  parked in `PrimTextures` until decoded), and `apply_prim_textures` uploads
  (deduped) the diffuse `Image` into each parked material's
  `base_color_texture`; a no-texture / failed face keeps its flat tint. The
  P5.2 shared placeholder material is gone (each face owns its material).
  **Terrain (P2.2) was migrated onto the same store**: `learn_composition` now
  calls `manager.request`, and its detail textures arrive as `TextureDecoded`
  (built with a tiling sampler) instead of the old
  `Command::FetchTexture` / `TextureReceived` + inline `decode_j2c`, so the
  viewer has one texture pipeline. New re-export: `CAP_GET_TEXTURE` from
  `sl-client-bevy`. Verified live on OpenSim (prims render textured, incl. the
  default plywood; terrain detail textures decode + tile; the on-disk cache
  populates under `~/.cache/sl-client-bevy-viewer/texturecache`).

## Phase 7 — Mesh objects

- [x] **P7.1. Mesh geometry.** For `SculptOrMeshKey::Mesh(_)`, fetch and
  decode the mesh **through the shared `sl_mesh::MeshStore`** — counterpart of
  the `TextureStore` the Phase 6 texturing drives (weak-ref dedupe,
  off-thread decode, Firestorm per-UUID `.mesh` disk cache, LOD-aware). Mirror
  the P6 `TextureManager` shape: a viewer `MeshManager` resource holding a
  `MeshStore` over a `BevyMeshFetcher` (cap URL from `SlCapabilities`;
  `GetMesh2` / `GetMesh`), fetch each mesh on a background `IoTaskPool` task,
  poll it, and announce it with a `MeshDecoded` message the object system
  reacts to. Do **not** decode on the render thread or drive the raw
  `Command::FetchMesh` / `MeshReceived` path — that is the low-level
  equivalent the Phase 6 texture work deliberately moved off of. Convert each
  decoded submesh with `to_bevy_mesh`, spawn one child entity per submesh, and
  texture it via the Phase 6 `face_material` / `TextureManager` path. Verify
  against the provisioned OpenSim mesh prim (`slclient-mesh.oar`). **Done — via
  the shared `MeshStore`, mirroring the P6 texture pipeline exactly.** A new
  `meshes.rs` module owns a `MeshManager` resource (a `MeshStore` over a
  `BevyMeshFetcher` whose `GetMesh2` / `GetMesh` cap URL is refreshed from
  `SlCapabilities`); each mesh is fetched on a background `IoTaskPool` task
  (blocking HTTP off-thread, decode on the store's own `rayon` pool at
  `MeshLod::FINEST`), and `poll_meshes` folds a finished decode into a shared
  cache and announces it with a `MeshDecoded` message. In `objects.rs` a mesh
  object requests its asset through the manager and, once the geometry is
  available (immediately if already cached, else when `apply_object_meshes`
  reacts to `MeshDecoded`), spawns one child entity per non-empty submesh via
  `to_bevy_mesh`, textured through the same Phase 6 `face_material` path — each
  submesh mapping to its Linden `TextureEntry` face slot (empty `NoGeometry`
  submeshes are skipped but still count as a face index). A mesh object waiting
  on its asset holds a `PendingMesh` (mesh key + the object's texture-entry
  bytes); the shared prim/mesh geometry build is routed through one
  `build_object_geometry` so a shape/category change rebuilds correctly. The
  mesh geometry stays in the object's local Second Life space; the object
  entity's `Transform` carries the object's scale / rotation / position and the
  single SL → Bevy basis change (mesh positions are dequantized to their
  normalized position domain, not pre-multiplied by scale — matching the core
  viewer unpack). New re-export: `CAP_GET_MESH` / `CAP_GET_MESH2` from
  `sl-client-bevy` (the mesh mirror of P6's `CAP_GET_TEXTURE`). Verified live
  on OpenSim: the provisioned mesh prim is classified, fetched over `GetMesh`,
  decoded off-thread, and its submesh entity spawned and textured; the on-disk
  cache populates under `~/.cache/sl-client-bevy-viewer/meshcache`. **Live
  finding + fix (shared with prims/terrain):** the shared `face_material` was
  switched from the P5.2 double-sided / culling-off placeholder to
  **single-sided (default back-face culling)** — Second Life renders a face
  only from its front, so a one-sided surface (a flat mesh quad, a prim cut
  face) must be invisible from behind rather than doubled. This is safe because
  the SL → Bevy basis change is a proper rotation (determinant `+1`, handedness
  preserved), so the outward windings that `sl_prim` tessellation and
  `sl_mesh` decode already produce stay front-facing under Bevy's CCW culling.
  Verified
  live: the provisioned flat mesh quad is now visible only from its front
  (top), and regular prims still render solid with no missing / inside-out
  faces.

## Phase 8 — `sl-sculpt` (sculpt-texture → geometry)

- [x] **P8.1. Map → grid.** The crate takes a decoded RGBA8 sculpt map
  (`sl_texture::DecodedImage`) + `sculpt_type` / flags and returns
  `sl_prim::PrimMesh`. Resample to a fixed working size (bilinear); pixel
  `(r, g, b) / 255 - 0.5` → a grid vertex. The crate itself stays I/O-free
  (like `sl-prim`): it never fetches or decodes. The `DecodedImage` it consumes
  must be sourced from the shared `TextureStore` (the same fetch /
  off-thread-decode / disk-cache pipeline the Phase 6 texturing drives), which
  the viewer supplies at P9.1. Do not add an inline JPEG-2000 decode here.
  Delivered as `tessellate(map, sculpt_type)` / `tessellate_with(map, params)`.
  `sl-texture` is depended on with `default-features = false` so the pure crate
  does not pull the OpenJPEG C dependency (only the `DecodedImage` type); the
  fixed working grid is `WORKING_SUBDIVISIONS = 32` quad cells per side
  (Firestorm's top sculpt LOD), bilinearly resampled per grid vertex.
- [x] **P8.2. Stitch modes.** Stitch per type — plane (no wrap), cylinder
  (wrap U), sphere (wrap U + collapse the pole rows), torus (wrap U + V); honour
  the mirror / invert flags (winding / normals). Build indices, per-vertex
  normals, and grid UVs; emit a single `PrimFace`. Fall back to a placeholder
  grid on a degenerate map (never panic). Seam / pole vertices are *shared* (one
  canonical vertex per lattice slot, wrapped edges fold to column / row `0`,
  pole rows collapse to a single vertex), so accumulated normals are smooth
  across them with no seam-wrapping pass. The flags follow Firestorm's
  `sculptGenerateMapVertices` — `reverse_u = invert XOR mirror` reverses the U
  sampling and `mirror` negates X — which, with one fixed triangle winding,
  compose to the four intended facings (so no separate winding flip). The
  degenerate fallback is a procedural sphere placeholder.
- [x] **P8.3. Stitch tests.** Unit tests per stitch type (counts; seam and pole
  vertices are shared, not duplicated). `cargo test -p sl-sculpt`. 14 tests:
  exact per-type vertex counts (plane `(N+1)²` > cylinder `N(N+1)` > torus `N²`
  > sphere `N²-N+2`), face integrity (parallel arrays, in-range whole triangles,
  unit normals, finite positions), degenerate + truncated fallback, and the
  mirror X-reflection.

## Phase 9 — Sculpt rendering in the viewer

- [x] **P9.1. Sculpt objects.** For `SculptOrMeshKey::Sculpt(texture_key)`,
  fetch + decode that sculpt map **through the same Phase 6 `TextureManager` /
  `TextureStore`** (request the texture id, react to its `TextureDecoded`, read
  the decoded `DecodedTexture` pixels as geometry input — reusing the store's
  fetch / off-thread-decode / disk-cache, not a fresh inline decode); the object
  stays in the "waiting on asset" state as a mesh does. Feed the pixels + type
  into `sl_sculpt`, convert with `to_bevy_prim_mesh`, and texture via Phase 6.
  **Done — mirroring the P7 mesh pipeline exactly, but keyed on the shared
  texture store.** A sculpted prim is classified `Sculpt` (already done since
  P5.1) and routed through `build_object_geometry`: it requests its sculpt map
  through the shared `TextureManager` (the same store the Phase 6 face textures
  use), and either stitches its face now (if the map is already decoded) or
  parks a pending sculpt build. A new `apply_object_sculpts` system reads the
  same `TextureDecoded` stream as `apply_prim_textures` — keying off a *pending
  sculpt build* rather than a parked face material, so the two consumers never
  contend — and on decode stitches the map with `tessellate_sculpt` into a
  single-face `PrimMesh`, spawning its face child (textured from `TextureEntry`
  slot 0) exactly as a plain prim's. The two deferred-build paths (mesh asset,
  sculpt map) were unified into one `PendingGeometry` enum on `TrackedObject`,
  and the prim / sculpt face spawn loop factored into one shared helper
  `spawn_prim_faces` (`build_prim_faces` and `build_sculpt_faces` differ only in
  how they produce the `PrimMesh`). New `sl-client-bevy` re-exports:
  `tessellate_sculpt` (the
  `sl_sculpt::tessellate` aliased so it does not collide with `sl_prim`'s
  `tessellate`) + `SculptParams` / `SculptStitch`, and the `sl-sculpt` dep — the
  sculpt mirror of P4's prim re-exports. Verified live on OpenSim (a provisioned
  sphere-sculptie prim renders as a textured sphere).

## Phase 10 — Avatar placeholders

- [x] **P10.1. Spheres.** Track avatars from `ObjectAdded` (pcode 47) and
  `CoarseLocationUpdate`; render each as a ~2 m UV-sphere `StandardMaterial` at
  the (converted) position; despawn on removal or when dropped from the coarse
  locations. No rig, baked textures, or animation. Verify with a second
  logged-in avatar. **Done.** A new `avatars.rs` module owns an `AvatarState`
  resource keyed by `AgentKey`, fed by two independent systems chained after the
  object/texture pipeline: `update_avatar_objects` folds the `ObjectAdded` /
  `ObjectUpdated` / `ObjectRemoved` stream for `pcode == 47` objects (the
  precise, per-frame source — including the agent's own avatar) into one
  placeholder sphere per avatar, and `update_coarse_avatars` renders a sphere
  for every *coarse-only* avatar in each `CoarseLocationUpdate` (one already
  tracked as a full object is skipped, and the agent's own `you` entry is left
  to the object path), despawning a coarse sphere the moment its avatar drops
  from the list. A full object supersedes a coarse dot for the same agent. Both
  sources share one lazily-built ~2 m UV-sphere mesh + soft-blue material; the
  spheres are plain world-space marker entities (not the avatar object root, so
  they are not scaled by the avatar's bounding box and carry no attachment
  children — attachment parenting stays with the object entity in `objects.rs`,
  unchanged). The spheres sit in the root region's frame like `objects.rs` (no
  multi-region origin offset yet). New re-export: `CoarseLocation` from
  `sl-client-bevy`. Verified live on OpenSim with a second avatar (a
  `sl-repl-tokio` login of `Friend Tester`): the viewer spawns a sphere for its
  own avatar and one for the second avatar. **Added on user request (beyond the
  base sphere spec):** a floating **name tag** per avatar — a `bevy_ui` text
  node anchored bottom-centre over the sphere each frame by projecting the
  sphere's head point with `Camera::world_to_viewport` (centred via the tag's
  `ComputedNode` size), hidden when off-screen / behind the camera. Names
  resolve once per agent through a `UUIDNameRequest`
  (`Command::RequestAvatarNames` → `Event::AvatarNames`) and are held in a small
  per-agent name cache (plus an "already requested" set) so a frequently-updated
  avatar is never re-requested; the tag shows a short id fragment until the real
  legacy name arrives. New re-export: `AvatarName` from `sl-client-bevy`.
  Verified live: the two tags resolve to `Avatar Tester` and `Friend Tester` and
  render centred over their spheres (user-confirmed).

## Phase 11 — Chat overlay

- [x] **P11.1. On-screen chat.** A `bevy_ui` `Text` node pinned to a corner; on
  `ChatReceived` append `"{from_name}: {message}"` (shout / whisper as a prefix
  label), keep the last N lines bottom-up. Read-only, no input box. Verify with
  chat from the second avatar. **Done.** A new `chat.rs` module owns a
  `ChatOverlay` resource (a bounded `VecDeque` of the last `CHAT_HISTORY_LINES`
  = 12 formatted lines) and one persistent overlay text node, tagged
  `ChatOverlayText`, spawned by a `setup_chat_overlay` startup system anchored
  at the bottom-left corner (`PositionType::Absolute`, `left`/`bottom`
  inset) so the node grows upward and the newest line sits at the bottom.
  `update_chat_overlay` folds every `SlSessionEvent::ChatReceived` message
  (`ChatFromSimulator`) into the history and rewrites the node's `Text` only
  when a displayable line arrives. Each line is
  `"{from_name}: {message}"`, with a `[whisper]` / `[shout]` prefix label for
  those two volumes and none for a normal say; the simulator already supplies
  the speaker's display name, so (unlike the avatar name tags) no
  `UUIDNameRequest` resolution is needed. Typing triggers
  (`StartTyping` / `StopTyping`, which actually arrive as
  `SlSessionEvent::ChatTyping` rather than `ChatReceived`) and empty-text
  messages are filtered so blank lines never accumulate. Viewer-only, no
  library change: `ChatMessage`, `ChatType`, and the other chat value types
  were already re-exported from `sl-client-bevy`.
  Verified live on OpenSim with a second avatar (a `sl-repl-tokio` login of
  `Friend Tester` co-located in the Default Region): the viewer rendered all
  three volumes correctly — `Friend Tester: hello from Friend Tester`,
  `[whisper] Friend Tester: psst over here`, and
  `[shout] Friend Tester: HELLO EVERYONE` — and the lines persist in the corner
  (user-confirmed).

The remaining phases replace the placeholder avatar spheres (Phase 10) with real
avatars: the system-avatar body, server- and client-side baked texturing (incl.
alpha), attachments, rigged mesh with bake-on-mesh, animations, and HUD
attachments. They follow the same top-to-bottom, one-point-per-session cadence.

A new CLI flag `--viewer-assets <dir>` is added in P13.2 and reused by every
avatar / animation phase; absent it, avatars keep the Phase-10 sphere. The
standard Linden `character/` assets (`avatar_skeleton.xml`, `avatar_lad.xml`,
base-body `.llm` meshes, visual-param definitions, the built-in animation
library) are client-side viewer files, not fetched from the grid — the viewer
reads them from that path (point at an installed Firestorm / SL viewer), and the
pure crates stay I/O-free (parse from `&[u8]` / `&str`), mirroring `sl-mesh` /
`sl-texture`. Pure-crate phases verify with `cargo test -p <crate>` using small
committed **fixture** XML / `.llm` / `.anim` files (deterministic-fixture style,
as in `sl-mesh` — not the full LL assets, which stay runtime-loaded); viewer
phases verify with a live run: OpenSim first, then aditi (real SL) for the paths
OpenSim can't exercise (server-side bake, BoM, HUDs).

Key net-new library facts (reused across the phases): `sl-proto` already carries
`AvatarAppearance { texture_entry, visual_params, cof_version, attachments, .. }`
and `PlayingAnimation`, the baked-slot constants
`avatar_texture::{HEAD,UPPER,LOWER,EYES,SKIRT,HAIR,LEFT_ARM,LEFT_LEG,AUX*}_BAKED`
(`COUNT = 45`), `decode_texture_entry`, `WearableType::Alpha`, and the
`AttachmentPoint` enum (HUD points 31–38). `sl-mesh` already decodes rigged-mesh
skin data (`MeshSkin` joint names / inverse-bind / bind-shape / alt-bind /
`pelvis_offset` + per-vertex `VertexWeights`), so rigged mesh needs skinning
*math*, not a new decoder. The BoM magic `IMG_USE_BAKED_*` UUID constants live
only in Firestorm today and are added to `sl-proto` in P17.3.

## Phase 12 — `sl-avatar`: skeleton & base body (pure crate)

- [x] **P12.1. Scaffold `sl-avatar`.** New crate (`edition = "2024"`,
  `publish = false`, `[lints] workspace = true`), `CHANGELOG.md`, `cliff.toml`
  (`tag_pattern ^sl_avatar_[0-9.]*$`), registered in the root `members`. Stub
  `lib.rs`; green `cargo build --workspace`. Mirror P0.
- [x] **P12.2. Skeleton parse.** `skeleton.rs`: parse `avatar_skeleton.xml`
  (from `&str`) → `Skeleton { joints }` with hierarchy, rest pos/rot/scale,
  pivot, and collision volumes; plus the attachment-point→joint map and HUD-
  point set from `avatar_lad.xml` `<attachment_point>`. Accessor helpers over
  indices (restriction lints). Committed minimal fixture skeleton for tests.
- [x] **P12.3. Base-mesh `.llm` decode.** `basemesh.rs`: decode the legacy
  Linden avatar mesh format → `BaseMesh { positions, normals, uvs, weights }`
  (per-vertex skin weights to skeleton joints) + the mesh's morph-target deltas.
  One decoder per base part (head, upper, lower, eyes, hair, skirt, eyelashes)
  with their LOD chains. Distinct from `sl_mesh` (`LLMesh`). **Done:**
  `BaseMesh::from_bytes` decodes a full base part (`lod="0"`) from `&[u8]` —
  header transform + flags, per-vertex positions/normals/binormals/primary
  (and optional detail) UVs, the per-vertex `VertexSkinWeight` (the single
  on-disk weight float split into `{ joint, blend }` where `joint = floor(w)`
  indexes the mesh's own skin-joint name table and `blend = w - joint` lerps to
  `joint + 1`), triangle faces, the joint-name table, the `MorphTarget` deltas
  (sparse per-vertex position/normal/binormal/UV deltas, read until the
  `End Morphs` sentinel), and the `SharedVertex` remap table.
  `LodMesh::from_bytes` decodes a reduced LOD (`lod="1"`..`"5"`): the same
  binary shape but only the header transform + the reduced face list are
  meaningful (faces index into the base part's vertices), so `vertex_count` is
  one-past-the-largest referenced index. A forward-only `Cursor` reads
  little-endian primitives via `f32::from_bits` / byte-fold shifts (the crate
  lints forbid `from_le_bytes` and `as`). Follows Firestorm
  `LLPolyMeshSharedData::loadMesh` / `LLPolyMorphData::loadBinary`. Committed
  binary fixtures (`mini_basemesh.llm` 4 verts / 2 faces / 2 joints / 1 morph /
  1 remap, `mini_basemesh_lod.llm`); `cargo test -p sl-avatar` (6 new tests).
- [ ] **P12.4. `avatar_lad.xml` params.** `params.rs`: parse the visual-param
  table — id, group, min/max/default, and each param's effect (`param_morph`
  mesh delta ref, `param_skeleton` bone scale/offset, driver→driven links).
  Produce a `VisualParams` model that maps an `AvatarAppearance.visual_params:
  Vec<u8>` (quantized 0–255, viewer order) onto typed param values.
- [ ] **P12.5. Tests.** Skeleton hierarchy + attachment/HUD point maps; `.llm`
  decode non-degenerate counts + weight normalization; param-table lookups and
  byte→value dequantization. `cargo test -p sl-avatar`.

## Phase 13 — Base avatar in the viewer (replace spheres)

- [ ] **P13.1. Bevy skinned-mesh conversion.** In `sl-client-bevy`: build a
  per-avatar Bevy skeleton instance (joint entity hierarchy + `SkinnedMesh`
  inverse bindposes) from `sl_avatar::Skeleton`, and `to_bevy` for each base-
  body part → a `Mesh` with `JOINT_INDEX` / `JOINT_WEIGHT` attributes. Add the
  `sl-avatar` dep + re-exports (`Skeleton`, `BaseMesh`, `VisualParams`,
  `AvatarAppearance`). Mirror P4.
- [ ] **P13.2. Un-morphed rigged body.** `--viewer-assets <dir>` CLI flag; load
  the `character/` assets once into an `AvatarAssetLibrary` resource (skeleton +
  base meshes + params), reading files here (crate stays I/O-free). In
  `avatars.rs`, for each `pcode == 47` object spawn the rigged base body (all
  parts) skinned to a fresh skeleton instance in the **default (un-morphed) rest
  shape**, replacing the placeholder sphere; keep the sphere as fallback when no
  assets / load fails, and keep the name tags. Verify a body renders on OpenSim.
- [ ] **P13.3. Visual-param morph targets.** Apply
  `AvatarAppearance.visual_params` (defaults where absent) → blend the base
  meshes' morph-target deltas so the body takes its real shape (face, weight,
  muscle, etc.). Re-morph on appearance update. One feature on top of P13.2.
- [ ] **P13.4. Skeletal-scale & driver params.** Apply `param_skeleton` bone
  scale/position params and driver→driven propagation so proportions (height,
  limb/head scale, pelvis) match; rebuild the skeleton instance's rest
  transforms accordingly. Verify a shaped avatar (2nd login) looks correct.

## Phase 14 — Server-published baked texturing (incl. alpha)

- [ ] **P14.1. Ingest `AvatarAppearance`.** In `avatars.rs`, on
  `Event::AvatarAppearance` decode `texture_entry`
  (`decode_texture_entry(_, avatar_texture::COUNT)`), read the baked-slot UUIDs
  (`avatar_texture::*_BAKED`), and request each through the shared
  `TextureManager` / `TextureStore` (the Phase-6 pipeline). Track per-avatar.
  (On SL these come from the server "Sunshine" bake; on OpenSim they come from
  *other* avatars' viewers' client-side bakes — either way they are published
  baked UUIDs we just fetch.)
- [ ] **P14.2. Map bakes onto body regions.** Build one `StandardMaterial` per
  base-body region from its baked slot (head→head, upper→upper body, lower→lower
  body, eyes→eyes, hair→hair, skirt→skirt), uploaded via the same
  `TextureDecoded` path as `apply_prim_textures`. Verify a textured other-avatar
  body on both grids.
- [ ] **P14.3. Alpha.** Baked textures carry the alpha wearables composited into
  their alpha channel; render body-region materials with `AlphaMode::Blend` (or
  `Mask`) so alpha'd regions turn invisible — essential so a worn mesh body's
  underlying system body is hidden. Fully-transparent region → hide that part.
- [ ] **P14.4. Refresh on rebake.** Re-request bakes on `RebakeAvatarTextures`
  and on a newer `cof_version` in a later `AvatarAppearance`.

## Phase 15 — Client-side baking (`sl-bake`, the OpenSim/legacy path)

The server-published path (Phase 14) covers *other* avatars on both grids, and
our *own* avatar on SL. It does **not** cover our own avatar on OpenSim (and any
grid without server bake): those grids expect the *client* to composite the bake
from wearable layers (legacy `UploadBakedTexture`). Without it our own avatar is
an untextured cloud. This phase composites the bake ourselves, primarily for our
own avatar and as the fallback whenever a baked slot is absent / default.

- [ ] **P15.1. Scaffold `sl-bake` + region compositing.** New pure crate
  (scaffold like P12.1; `sl-texture` dep with `default-features = false`). Given
  the ordered per-region layers (skin → tattoo → clothing → alpha mask) as
  decoded `DecodedImage`s + their params (tint colour, alpha, tex-gen),
  composite each bake region (head/upper/lower/eyes/skirt/hair) into a baked
  RGBA. Alpha layers carve the alpha channel. Tests over synthetic layers.
  `cargo test -p sl-bake`.
- [ ] **P15.2. Wearable layer inputs.** Read the agent's worn wearables
  (`AgentWearables` / the COF), fetch each wearable **asset** (skin / tattoo /
  clothing / alpha) to get its layer texture ids + tint (which visual params
  colour a layer, e.g. skin tone), and decode the layer textures through the
  shared `TextureManager`. Assemble the per-region layer lists `sl-bake` needs.
- [ ] **P15.3. Composite & render our own bake.** When no server bake is
  published for an avatar (our own on OpenSim), composite its regions with
  `sl-bake` and drive the Phase-14 body-region materials + Phase-17 BoM from the
  local composite instead of a fetched baked UUID (alpha honoured). Verify our
  own avatar renders skin/clothing-textured on OpenSim.
- [ ] **P15.4. (Optional) Publish the bake.** J2C-**encode** the composited
  regions and upload via the existing `UploadBakedTexture` cap so the sim /
  other viewers see us. **Needs a J2C encoder** (OpenJPEG encode) — the one
  heavy net-new dependency; may slip to a follow-up. Local rendering (P15.3)
  does not depend on it.

## Phase 16 — Attachments (rigid)

- [ ] **P16.1. Detect & parent.** In `objects.rs` `reconcile_parent`, when an
  object's `parent_id` resolves to a **pcode-47 avatar** (not a prim linkset),
  decode `attachment_point()`, look up that avatar's skeleton **joint entity**
  (Phase 13), and parent the attachment there via `ChildOf` so it follows the
  posed skeleton. Hold-pending when the avatar/joint is not present yet (reuse
  the existing pending-adoption path).
- [ ] **P16.2. Attachment transform.** Place the attachment at its stored local
  offset/rotation relative to the joint; honour attachment `ADD_FLAG` vs
  replace. Verify a rigid prim/mesh attachment (e.g. a worn hat) tracks the
  avatar on OpenSim.

## Phase 17 — Rigged mesh & bake-on-mesh

- [ ] **P17.1. Skinning math.** In `sl-avatar` `skin.rs`: a matrix-palette
  helper taking `sl_mesh::MeshSkin` (joint names + inverse-bind + bind-shape +
  alt-bind + `pelvis_offset` + `lock_scale_if_joint_position`) and per-vertex
  `VertexWeights` against a `Skeleton` instance's current joint world transforms
  → skinned vertices (≤4 weights). Tests with a synthetic skeleton.
- [ ] **P17.2. Rigged-mesh rendering.** A mesh object with a skin block worn on
  an avatar renders as a Bevy `SkinnedMesh` bound to that avatar's skeleton
  instance (not a static child), so mesh bodies/clothing deform with the avatar.
  Reuse the `MeshManager` fetch/decode; join to the avatar via the Phase-16
  attachment association.
- [ ] **P17.3. Bake-on-mesh.** Add the `IMG_USE_BAKED_*` magic UUID constants to
  `sl-proto` (+ slot↔UUID map). In the viewer, when a face's
  `TextureFace.texture_id` equals a BoM magic UUID, texture that face with the
  wearer's corresponding **baked** avatar texture — the server-published bake
  (Phase 14) or the client-side composite (Phase 15) — instead of fetching,
  honouring alpha. This is what makes modern mesh bodies show the avatar's skin.
  Verify a BoM mesh body on aditi (server bake) and on OpenSim (client bake).

## Phase 18 — Animations (full pipeline)

- [ ] **P18.1. Scaffold `sl-anim` + `.anim` decode.** New pure crate (scaffold
  like P12.1). `motion.rs`: decode the Linden keyframe-motion binary → `Motion`
  with per-joint rotation/position keyframe tracks, priority, ease-in/out, loop
  points, and constraints. Fixture-based tests. `cargo test -p sl-anim`.
- [ ] **P18.2. Built-in animation library.** Resolve an `anim_id` to its asset:
  built-in fixed-UUID motions from the `--viewer-assets` path, else fetch an
  uploaded `.anim` over `ViewerAsset` (reuse the asset fetch path). Cache
  decoded motions.
- [ ] **P18.3. Drive the skeleton.** On `Event::AvatarAnimation`, for each
  `PlayingAnimation` sample its `Motion` each frame and pose the target avatar's
  skeleton-instance joints (via a `sl-client-bevy` animation driver / Bevy
  clip). Attachments (Phase 16) and rigged mesh (Phase 17) follow automatically.
  Verify a walking/waving avatar.
- [ ] **P18.4. Priority blending.** Resolve concurrently-playing animations
  per-joint by priority with ease-in/out transitions (higher priority wins a
  joint, blend on start/stop). Verify layered animations (e.g. an AO stand + a
  gesture) compose correctly.

## Phase 19 — HUD attachments

- [ ] **P19.1. Detect HUD.** Classify an attachment whose `attachment_point()`
  is a HUD slot (31–38, `HudCenter` / `HudTopLeft` / …); route it out of the
  world scene to a dedicated screen-space HUD layer, and only for the **agent's
  own** attachments.
- [ ] **P19.2. HUD rendering.** Render HUD-attached prims/mesh on a HUD camera /
  render layer anchored per the HUD attachment-point screen layout (orthographic
  / screen-relative), reusing the existing prim/mesh geometry+texture build.
  Verify a simple HUD renders fixed to the screen on aditi.

## Phase 20 — Aditi (real SL) verification

OpenSim end-to-end and the clippy / fmt / `rumdl` clean sweep are **not** a
separate phase — they happen inside every phase above as it builds, live-tests,
and commits (per the Legend). What OpenSim can't exercise is the SL-only
appearance stack, so this final phase is the real-SL pass:

- [ ] **P20.1. Aditi (real SL).** Run against `credentials.aditi.toml` + the MFA
  wrapper for the SL-only paths OpenSim can't exercise: **server-side**-baked
  bodies (vs. OpenSim's client-side bake), BoM mesh bodies with alpha, and the
  agent's HUDs.

---

## Non-goals (deferred; candidate follow-up roadmaps)

Advanced materials (PBR / GLTF `GltfMaterialOverride`, legacy normal / specular
`RenderMaterials`, bump / shiny / glow / fullbright), avatar cloth / flexi /
breast-butt physics params, facial-morph lip-sync, flexi / particles / lights /
reflection probes, water surface, sky / atmosphere, distance-based LOD switching
(fixed High LOD), object selection / interaction, any chat *input* or non-quit
UI, and sound. Client-side baking *is* in scope (Phase 15) for local rendering;
only the J2C-**encode** + re-upload of our own bake via `UploadBakedTexture` (so
*other* viewers see us) may slip to a follow-up, since it needs an OpenJPEG
encoder the stack does not have yet.
