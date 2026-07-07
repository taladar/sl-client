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
- **`sl-j2c-encode`** — new crate (no Bevy, no I/O): an in-memory JPEG-2000
  (`.j2c`) **encoder** for canonical RGBA8, built on the OpenJPEG C library
  (`openjpeg-sys`) — deliberately the *same* backend `jpeg2k` decodes with, so
  only one OpenJPEG is linked (the pure-Rust `openjp2` port would export
  duplicate `#[no_mangle]` `opj_*` C symbols that collide at link time). It is
  the *only* workspace crate that owns `unsafe` FFI (so the rest keeps
  `unsafe_code = "forbid"`); `sl-texture`'s `encode` feature wraps it as
  `encode_j2c(&DecodedImage)`. Added in P15.4 to publish a client-side bake.
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
- [x] **P12.4. `avatar_lad.xml` params.** `params.rs`: parse the visual-param
  table — id, group, min/max/default, and each param's effect (`param_morph`
  mesh delta ref, `param_skeleton` bone scale/offset, driver→driven links).
  Produce a `VisualParams` model that maps an `AvatarAppearance.visual_params:
  Vec<u8>` (quantized 0–255, viewer order) onto typed param values. **Done:**
  `VisualParams::from_xml` collects every `<param>` anywhere in the document
  (skeleton / mesh / layer-set / driver sections), deduplicating by id (last
  definition wins, mirroring `addVisualParam`'s map overwrite) and sorting by
  ascending id. Each `VisualParam` carries `{ id, group, name, label, wearable,
  sex, min, max, default, effect }`, where `ParamEffect` is one of `Morph`
  (target resolved later by name in the base-mesh morph table),
  `Skeleton(Vec<BoneOffset>)` (per-bone `scale` + optional `offset`),
  `Driver(Vec<DrivenParam>)` (each with the `min1/max1/max2/min2` trapezoid
  thresholds, absent ones defaulting to the driver's own bounds), `Color`
  (RGBA ramp) or `Alpha` (bake inputs kept so they still occupy wire slots),
  or `None`. `ParamGroup::is_transmitted` selects the wire subset (Tweakable
  `0` + TransmitNotTweakable `3`); the reference viewer packs those **sorted by
  id** because it iterates a `std::map<S32, LLVisualParam*>` in key order, so
  `VisualParams::transmitted()` is exactly the wire order and
  `map_appearance(&[u8])` dequantizes byte `i` against the `i`-th transmitted
  param via Firestorm's `U8_to_F32` ramp (with its snap-to-zero step), leaving
  short-vector tail params at their default. Committed fixture
  `mini_params.xml` (one param of each effect type + a non-transmitted group-1
  param, ids out of document order to exercise the id sort); `cargo test -p
  sl-avatar` (9 new tests). LIVE-VALIDATED against the real (uncommitted)
  Firestorm `avatar_lad.xml`: 672 distinct params, **253 transmitted** (the
  known SL wire count), every param resolving to a recognized effect
  (morph 223 / skeleton 83 / driver 164 / color 108 / alpha 94, none 0); first
  wire ids `Big_Brow`(1)/`Nose_Big_Out`(2)/`Broad_Nostrils`(4)…, and the
  `Male_Skeleton`(32) param carrying 22 skeletal bones.
- [x] **P12.5. Tests.** Skeleton hierarchy + attachment/HUD point maps; `.llm`
  decode non-degenerate counts + weight normalization; param-table lookups and
  byte→value dequantization. `cargo test -p sl-avatar`. **Done:** the P12.2–
  P12.4 modules each already ship their own `#[cfg(test)]` unit tests over the
  private surface; this adds `tests/avatar.rs`, an *integration* suite that
  drives only the re-exported public API (`sl_avatar::*`) an external consumer
  sees and asserts the structural invariants the three bullets call out rather
  than fixed fixture values: the skeleton is a coherent tree (single parentless
  root, every parent index precedes its child, each child listed once under its
  parent) with round-tripping name/alias lookups; the attachment map, per-point
  `is_hud`, `hud_points()`, and the wire enum's own `AttachmentPoint::is_hud`
  all agree, and a shared joint (`mChest`) proves the cross-asset lad→skeleton
  reference resolves; the base `.llm` has non-degenerate counts with every
  per-vertex stream one-entry-per-vertex, all face / morph-delta / shared-vertex
  indices in range, one skin weight per vertex whose joint indexes the mesh's
  own joint table and whose blend is normalized to `[0, 1)` (the last joint
  never blends past the table), and a reduced LOD whose `vertex_count` is
  exactly its max referenced index + 1; the param table is strictly id-sorted
  with id lookups round-tripping, `transmitted()` is exactly the wire-carrying
  groups (length matching `transmitted_count()`, complement covering the rest),
  and a full appearance vector dequantizes so that `AppearanceValues::weight`
  matches each param's own `weight_from_byte` slot-for-slot and stays within the
  param's min/max, with empty / short vectors falling back to defaults and
  recording no raw byte. The `clippy::tests_outside_test_module` restriction
  lint applies to `tests/` targets too, so the suite lives in a `#[cfg(test)]
  mod tests`. 10 integration tests (21 unit + 10 = 31 total green).

## Phase 13 — Base avatar in the viewer (replace spheres)

- [x] **P13.1. Bevy skinned-mesh conversion.** In `sl-client-bevy`: build a
  per-avatar Bevy skeleton instance (joint entity hierarchy + `SkinnedMesh`
  inverse bindposes) from `sl_avatar::Skeleton`, and `to_bevy` for each base-
  body part → a `Mesh` with `JOINT_INDEX` / `JOINT_WEIGHT` attributes. Add the
  `sl-avatar` dep + re-exports (`Skeleton`, `BaseMesh`, `VisualParams`,
  `AvatarAppearance`). Mirror P4. **Done:** new `avatars.rs` module, the
  system-avatar counterpart of `meshes.rs` / `prims.rs`.
  `to_bevy_base_mesh(&BaseMesh) -> Mesh` builds a `TriangleList` with
  position / normal / UV0 and, when the part is weighted, `JOINT_INDEX`
  (`Uint16x4`, named explicitly since `Vec<[u16; 4]>` has no unambiguous
  `Into<VertexAttributeValues>`) + `JOINT_WEIGHT` (`Float32x4`): the legacy base
  body binds each vertex between two *adjacent* joints in the part's own
  joint-name table, so only the first two of Bevy's four influence slots are
  used (`[joint, joint+1 clamped, 0, 0]` / `[1-blend, blend, 0, 0]`) and the
  joint indices are the part-local table order. `BevySkeleton::from_skeleton`
  converts the parsed skeleton into per-joint local rest `Transform`s, parent
  indices, and rest global (bind-pose) matrices — the data a joint-entity
  hierarchy is spawned from (the actual `commands.spawn` stays in the viewer at
  P13.2, so this module holds no `World` / `Commands`, mirroring how P4 returns
  a `Mesh` and P5 spawns). Rest rotations are the file's Euler XYZ **degrees**;
  `euler_deg_to_quat` reproduces Firestorm `mayaQ(x, y, z, XYZ)` (apply X, then
  Y, then Z), which in glam's column-vector convention is
  `qz.mul_quat(qy).mul_quat(qx)`. Transforms/geometry stay in Second Life Z-up
  space (the viewer applies the axis change once at the avatar root, as terrain
  and object meshes do). `BevySkeleton::base_mesh_skin(&BaseMesh)` resolves a
  part's joint-name table against the skeleton into a `BaseMeshSkin`
  (skeleton joint indices + parallel inverse bindposes) the viewer feeds into a
  `SkinnedMesh`, returning `None` if any joint name is absent.
  `cargo test -p sl-client-bevy` (6 new unit tests, reusing `sl-avatar`'s
  committed `mini_skeleton.xml` / `mini_basemesh.llm` fixtures via
  `include_str!` / `include_bytes!`): joint/root/parent + alias round-trip,
  bind-pose translation composing down the hierarchy, a 90°-yaw Euler check,
  one-per-vertex skin attributes with the two-slot partition-of-unity weights,
  cross-asset skin resolution, and the missing-joint `None`.
- [x] **P13.2. Un-morphed rigged body.** `--viewer-assets <dir>` flag; load
  the `character/` assets once into an `AvatarAssetLibrary` resource (skeleton +
  base meshes + params), reading files here (crate stays I/O-free). In
  `avatars.rs`, for each `pcode == 47` object spawn the rigged base body (all
  parts) skinned to a fresh skeleton instance in the **default (un-morphed) rest
  shape**, replacing the placeholder sphere; keep the sphere as fallback when no
  assets / load fails, and keep the name tags. Verify a body renders on OpenSim.
  **Done:** new viewer module `avatar_assets.rs` owns the disk read — the
  `--viewer-assets <dir>` flag (env `SL_VIEWER_ASSETS`) points at an installed
  Firestorm / Second Life `character/` directory, and
  `AvatarAssetLibrary::load` (via `fs_err`, the workspace-sanctioned reader)
  parses `avatar_skeleton.xml` → `BevySkeleton`, `avatar_lad.xml` →
  `VisualParams` (kept for the P13.3 / P13.4 morph phases), and the eight
  `lod = 0` base-part `.llm` files named by the `avatar_lad.xml` `<mesh>`
  table (head, hair, eyelashes, upper body, lower body, skirt, and the two
  eyeballs). Each part's skeleton binding is resolved at load and a part whose
  binding is unresolvable is skipped (logged), not fatal: the six weighted
  parts resolve their own joint-name table against the skeleton into a
  `BaseMeshSkin` (`Skinned`), while `avatar_eye.llm` carries **no** skin
  weights and no joint table, so each eyeball is bound `Rigid` to a single eye
  joint (`mEyeLeft` / `mEyeRight`) and simply follows it. A load failure or an
  absent flag logs and leaves avatars as Phase-10 spheres. A Startup system
  (`setup_avatar_body`) builds the per-avatar-**invariant** render assets once
  into an `AvatarBody` resource — one shared Bevy `Mesh` per part (via the
  P13.1 `to_bevy_base_mesh`), one shared `SkinnedMeshInverseBindposes` per
  skinned part, one shared skin `StandardMaterial`, and the joint rest
  transforms / parent indices a fresh skeleton instance is spawned from. In
  `avatars.rs`, `apply_object` now spawns, per full-object avatar, a body-root
  anchor entity carrying the single Second Life → Bevy basis change, a fresh
  joint-entity hierarchy under it, a `SkinnedMesh` per skinned part (its
  `joints` mapped from the part's `JOINT_INDEX` table to this instance's joint
  entities) parented to the root, and each rigid eyeball parented to its eye
  joint entity. Because Bevy skinning derives each vertex's world position
  solely from the joint `GlobalTransform`s (`world_from_local =
  skin_model(...)`, ignoring the mesh entity's own transform), the axis change
  carried by the root joints lands the Second-Life-space geometry correctly in
  Bevy's Y-up world with no per-mesh transform. The root is lowered by the
  pelvis rest height so the pelvis sits at the reported object position (Second
  Life reports an avatar near its pelvis); moving an avatar re-applies that
  transform, and the name tag now floats at a fixed head height over a
  generalized `AvatarAnchor` (sphere or body root) rather than the old
  sphere-only marker. Coarse-only (minimap) avatars stay spheres — only full
  objects get bodies. Net-new library change was only three `sl-avatar`
  error-type re-exports from `sl-client-bevy` (`SkeletonError` / `ParamError` /
  `BaseMeshError`) for the loader's error enum; `cargo test -p
  sl-client-bevy-viewer` gains a `body_root_transform` planting test (24 total
  green). Verified live on OpenSim (Default Region, user-confirmed on screen):
  an **untextured default "Ruth" avatar in the T-pose** rest shape replaces the
  placeholder sphere — no skinning / wgpu validation errors, the skinned body
  rendering in bind pose exactly as authored.
- [x] **P13.3. Visual-param morph targets.** Apply
  `AvatarAppearance.visual_params` (defaults where absent) → blend the base
  meshes' morph-target deltas so the body takes its real shape (face, weight,
  muscle, etc.). Re-morph on appearance update. One feature on top of P13.2.
  **Done:** new pure `sl-avatar` module `morph` — `MorphWeights` resolves a
  wire `visual_params` byte vector against the `VisualParams` table into a
  `morph-target name → weight` lookup (only `param_morph`-effect params,
  weighted from the appearance vector or their default; non-morph colour /
  alpha / skeletal params never move geometry), built once per avatar and
  reused across every base part; `MorphWeights::apply(&BaseMesh) -> MorphedMesh`
  blends the part's morph-target deltas exactly as Firestorm
  `LLPolyMorphTarget::apply` — `position += weight * delta` and
  `normal = normalize(base + Σ weight * delta * 0.65)` (the
  `NORMAL_SOFTEN_FACTOR`), producing morphed positions + normals in Second Life
  Z-up space (UV / binormal deltas are silhouette-neutral and left to the base,
  matching what the un-textured body needs). Driver → driven propagation stays
  deferred to P13.4, so a morph param not directly transmitted sits at its
  default. In `sl-client-bevy`, `to_bevy_base_mesh` is refactored onto a shared
  builder and joined by `to_bevy_morphed_mesh(&BaseMesh, &MorphedMesh)` —
  identical UV / skin / index data over the morphed positions / normals, so a
  morphed mesh stays skin-compatible (same vertex count + `JOINT_INDEX` /
  `JOINT_WEIGHT`) and a re-morph is a plain mesh swap on the same skeleton
  instance. In the viewer, each rigged base-part entity now carries an
  `AvatarBodyPart { agent, part }` marker, and a new `apply_avatar_morphs`
  system caches each avatar's latest `visual_params` vector and, on a fresh
  appearance or a just-spawned body part (`Added<AvatarBodyPart>`), rebuilds
  that avatar's part meshes from the resolved `MorphWeights` — deferred and
  idempotent so an appearance that arrives before the body still lands, and a
  newer appearance re-morphs. Net-new library surface was three re-exports
  (`MorphWeights`, `MorphedMesh`, `to_bevy_morphed_mesh`) plus the `sl-avatar`
  module. Verified live on OpenSim: the agent's own `AvatarAppearance` arrives
  and all 8 base parts morph (`morphed 8 body part(s) across 1 avatar(s)`) with
  no skinning / wgpu errors, the rigged body re-shaping from its real
  transmitted visual params.
- [x] **P13.4. Skeletal-scale & driver params.** Apply `param_skeleton`
  bone scale/position params and driver→driven propagation so proportions
  (height, limb/head scale, pelvis) match; rebuild the skeleton instance's
  rest transforms accordingly. Verify a shaped avatar (2nd login) looks correct.
  **Done:** two new pure `sl-avatar` modules. `resolve` — `ResolvedParams` turns
  a partial appearance vector into every param's effective weight: it fills in
  the *non-transmitted* driven params from their (transmitted) drivers via the
  Firestorm `LLDriverParam::getDrivenWeight` trapezoid ramp (the classic
  transmitted `male` driver → the non-transmitted `Male_Skeleton` / `Male_Head`
  … params), leaves a transmitted driven param at its wire value (the sender
  already resolved it), decides avatar sex from the `male` param (`> 0.5`), and
  sex-gates each param's `effective_weight` (`getSex() & avatar_sex ? weight :
  default`, mirroring the gate the reference viewer applies before every
  distortion). `skeletal` — `SkeletalDeformations` sums `effective_weight *
  deformation` per bone into a scale + offset delta (the net of Firestorm
  `LLPolySkeletalDistortion::apply`, which telescopes from a zero baseline, so a
  param at any weight contributes `weight * deformation`; collision-volume
  `inheritScale` is skipped as it never touches the skinned skeleton). `morph`'s
  `MorphWeights` now routes through `ResolvedParams` too (new `from_resolved`),
  so driven morphs and sex gating apply to P13.3 shapes as well. In
  `sl-client-bevy`, `BevySkeleton` gains `deformed_local_transforms(&deform)`:
  because the Second Life skeleton has semantics a plain nested transform
  hierarchy cannot express — a bone's own scale stretches only its bound
  geometry (never inherited into a child's world scale) while a parent's *local*
  scale stretches its immediate child's position offset (the `scaleChildOffset`
  mechanism that drives height / limb length) — it runs that exact world-matrix
  recurrence and returns each joint's `parent_world⁻¹ · own_world` relative
  transform, which Bevy's ordinary propagation re-composes back into the correct
  world matrix regardless of how Bevy accumulates scale (the transmitted
  skeletal bones are axis-aligned, so the relatives carry no shear and decompose
  losslessly into a `Transform`); the rest bind poses / inverse bindposes are
  left untouched, so the deformation reads as the skin's deviation from bind
  pose. In the viewer, each skeleton-instance joint now carries an
  `AvatarJoint { agent, index }` marker, `apply_avatar_morphs` became
  `apply_avatar_appearance`
  (one `ResolvedParams` per dirty avatar feeds both the morph mesh rebuild and
  the joint re-deform), and a body's joints are re-set from
  `deformed_local_transforms` on the same fresh-appearance / just-spawned dirty
  signal the morphs use. Net-new library surface was three re-exports
  (`ResolvedParams`, `SkeletalDeformations`, `BoneDeform`) plus the two
  `sl-avatar` modules and the `BevySkeleton` method. Verified live on **both**
  grids: OpenSim (`shaped 8 body part(s) + 133 joint(s) across 1 avatar(s)`) and
  aditi with a genuinely shaped avatar (avatar1), each applying its morphs
  *and* its full 133-joint skeletal deformation with no skinning / wgpu errors.
  Driver→driven propagation of skeletal / morph params to *other* (non-agent)
  avatars still waits on their appearance arriving (P14 baked slots carry it),
  and a fully general SL skeleton under animation will need CPU world-matrix
  posing (the nested-relative shortcut holds only while the pose is static +
  shear-free), which the animation phase will revisit.
- [x] **P13.5. Conditional mesh-part visibility (whole-mesh show/hide).** The
  Firestorm `updateMeshVisibility` / `renderTransparent` mechanism, showing or
  hiding whole base-avatar mesh regions from what is worn so the body renders
  only the right parts. **Scope split:** narrowed at implementation to part
  **(a)**; part **(b)** clothing-morph alpha masks moved to **P14.5** because it
  genuinely needs the Phase-14 baked-texture alpha pipeline (Firestorm's
  per-vertex `maskWeight` comes from the baked texture's alpha channel via
  `onBakedTextureMasksLoaded`, not from geometry alone). **Done:** render the
  skirt part (`avatar_skirt.llm`) only when a skirt is worn — the reference test
  `isWearingWearableType(WT_SKIRT) && isTextureVisible(TEX_SKIRT_BAKED)`, which
  for another avatar reduces to the `TEX_SKIRT_BAKED` slot holding a real,
  non-`IMG_INVISIBLE` bake — and hide a whole base region (head / hair / eyes /
  upper / lower / skirt) when a worn attachment face carries the matching
  `IMG_USE_BAKED_*` magic UUID (a mesh body/clothing replacing that region); the
  default (no skirt, no mesh body) hides the skirt and shows every other region.
  Net-new library surface was in `sl-proto`'s `avatar_texture` module (already
  re-exported wholesale by both runtimes, so no per-runtime export churn): the
  `IMG_DEFAULT_AVATAR` / `IMG_INVISIBLE` / eleven `IMG_USE_BAKED_*`
  magic-texture UUID constants, an `is_bake_visible(TextureKey)` predicate (the
  `isTextureVisible` baked-slot test), and `use_baked_slot(TextureKey) ->
  Option<usize>` (a sentinel → baked slot mapping); `MAX_FACES` gained a
  re-export from both runtimes. In the viewer, each base part now carries a
  `BodyRegion` (`avatar_assets.rs`, keyed to its baked slot — eyelashes ride
  with the head, eyeballs with the eyes, matching the reference viewer),
  threaded onto the `AvatarBodyPart` marker. `AvatarState` gained per-agent
  skirt visibility
  (computed from each `AvatarAppearance`'s `TEX_SKIRT_BAKED` slot) plus
  lightweight attachment bookkeeping — a parent-scoped map and a once-scanned
  per-object `IMG_USE_BAKED_*` slot set for every non-root object — and a new
  `apply_avatar_part_visibility` system that each frame chases each
  `IMG_USE_BAKED`-bearing attachment up its linkset chain to its avatar root and
  sets each part's `Visibility` (only when it actually changed). The skirt
  spawns `Hidden` so an un-worn skirt never flashes. Verified live on OpenSim:
  our own
  skirt-less avatar logs `skirt not worn` and the base skirt mesh is hidden on
  screen (user-confirmed), the body still shaping (`shaped 8 body part(s) + 133
  joint(s)`) with no skinning / wgpu errors. The `IMG_USE_BAKED_*` region-hide
  cannot fire on a plain OpenSim avatar (no mesh body), so it is covered by unit
  tests (chain-attribution + sentinel scan) and Firestorm parity; it exercises
  live only near a mesh-body avatar (aditi / SL).

## Phase 14 — Server-published baked texturing (incl. alpha)

- [x] **P14.1. Ingest `AvatarAppearance`.** In `avatars.rs`, on
  `Event::AvatarAppearance` decode `texture_entry`
  (`decode_texture_entry(_, avatar_texture::COUNT)`), read the baked-slot UUIDs
  (`avatar_texture::*_BAKED`), and request each through the shared
  `TextureManager` / `TextureStore` (the Phase-6 pipeline). Track per-avatar.
  (On SL these come from the server "Sunshine" bake; on OpenSim they come from
  *other* avatars' viewers' client-side bakes — either way they are published
  baked UUIDs we just fetch.)
- [x] **P14.2. Map bakes onto body regions.** Build one `StandardMaterial` per
  base-body region from its baked slot (head→head, upper→upper body, lower→lower
  body, eyes→eyes, hair→hair, skirt→skirt), uploaded via the same
  `TextureDecoded` path as `apply_prim_textures`. Verify a textured other-avatar
  body on both grids.

  **Done (P14.1 + P14.2 bundled).** `ingest_avatar_bakes` reads the six
  base-body baked slots (`BODY_BAKE_SLOTS`) from each `AvatarAppearance`'s
  `texture_entry`, keeps only the visible bakes
  (`avatar_texture::is_bake_visible`) via `visible_body_bakes`, requests each
  through the shared `TextureManager`, and tracks them per avatar in
  `AvatarState::baked_textures`. `assign_avatar_bake_materials` gives every base
  part a per-`(avatar, region)` `StandardMaterial` (new `AvatarBakeMaterials`
  resource) — deferred/idempotent like `apply_avatar_appearance` (dirty set +
  `Added<AvatarBodyPart>`), a region with no bake keeping the shared skin
  material; `apply_avatar_bake_textures` fills each material's
  `base_color_texture` (and resets `base_color` to white so the composited bake
  is untinted) as the bake decodes, mirroring `apply_prim_textures`. A body-part
  material query pushed the `Update` tuple past Bevy's 20-system cap, so the
  appearance/bake systems are nested into one sub-tuple.

  **Own-avatar bake trigger (net-new, beyond the listed items).** The viewer is
  a passive renderer, so on a central-baking grid our *own* avatar was never
  baked → an untextured cloud → nothing for P14 to fetch. New `appearance.rs`
  (`ServerBakeState` + `drive_server_bake`) drives the modern SL server-side
  bake: on seeing the `UpdateAvatarAppearance` cap it reads the current Current
  Outfit Folder version from the login-seeded inventory skeleton
  (`Command::QueryInventoryFolders` → `Event::InventoryFolders`, the same model
  the inventory cache is built on — `current_outfit_version`) and POSTs
  `RequestServerAppearanceUpdate { cof_version }`, retrying with the grid's
  `expected` version on a mismatch (bounded). Net-new library surface: a public
  `pub use sl_proto::CAP_UPDATE_AVATAR_APPEARANCE` re-export from
  `sl-client-bevy` (matching `CAP_GET_TEXTURE`). This is the
  `server-appearance-bake` conformance handshake, now driven from the viewer.

  **Verified live on aditi (SL):** the trigger read COF version 15, the grid
  accepted the bake in one attempt, our own `AvatarAppearance` then arrived with
  5 real bakes, and the body-region materials were assigned to 7 parts — the
  avatar body renders textured (user-confirmed on screen). Inert on OpenSim
  (no `UpdateAvatarAppearance` cap; our own OpenSim bake is the Phase-15
  client-bake gap).

  **`sl-texture` decoder fix (net-new, fell out of live verification).** Only
  *part* of the body (and some prims/terrain) textured at first: the store's
  full-resolution fetch stopped at the viewer's `1/8`-rate byte *estimate*
  (`Header::discard_data_size(0)` / `calcDataSizeJ2C`), which for a texture that
  compresses worse than 8:1 truncates the codestream mid-tile-part, so OpenJPEG
  rejects it (`jpeg2k` "Tile part length size inconsistent with stream length").
  The estimate is only a valid prefix boundary for *coarser* LODs. Fix:
  `TextureStore::upgrade` now decodes the fast estimate prefix first (unchanged
  for the well-compressing majority) and, only when that decode *fails* and the
  codestream is not yet complete, grows to a new `Header::full_data_size_bound`
  (the uncompressed-size upper bound — always enough) and decodes once more. So
  the rare failing texture recovers without slowing the common path (a first
  attempt to always-fetch-full made *every* texture pull ~8× the bytes and
  crawled — reverted). Verified live on aditi: 299 texture decodes in 90 s (was
  ~52 under the always-full attempt), the single truncating texture recovered by
  retry, avatar + scene textured. This is a shared `sl-texture` / `sl-proto`
  change benefiting all textures, not just avatar bakes.
- [x] **P14.3. Alpha.** Baked textures carry the alpha wearables composited into
  their alpha channel; render body-region materials with `AlphaMode::Blend` (or
  `Mask`) so alpha'd regions turn invisible — essential so a worn mesh body's
  underlying system body is hidden. Fully-transparent region → hide that part.

  **Done.** Each decoded bake is classified once (`classify_bake_alpha` →
  `BakeAlpha::{Opaque, Masked, Transparent}`, cached per texture id in
  `AvatarBakeMaterials::alpha`): a source with no alpha channel (`components < 4`,
  the decoder fills alpha opaque) or an all-opaque alpha is `Opaque`; a mix of
  kept and carved pixels is `Masked`; an all-carved alpha is `Transparent`. The
  0.5 mask cutoff is shared between the `AlphaMode::Mask` threshold and the u8
  classification cutoff (128). `apply_bake_image` now sets each region material's
  `alpha_mode` from its bake's class — `Opaque` (the cheap opaque pass, correct
  for plain skin) or `Mask(0.5)` (carved pixels discarded). `Mask` rather than
  `Blend` deliberately: an avatar body is mostly opaque, so masking keeps it in
  the depth-writing opaque pass and dodges transparency-sorting artifacts on the
  non-convex body, while still carving alpha'd pixels away. A wholly `Transparent`
  region is additionally hidden outright by `apply_avatar_part_visibility` (it now
  reads `AvatarBakeMaterials` and unions the alpha-transparent slot into the P13.5
  `IMG_USE_BAKED_*` hide) — so a worn mesh body's alpha layer hides the underlying
  system body even where no `IMG_USE_BAKED_*` sentinel signalled it. Unit-tested;
  no library-surface change (viewer-internal). Live-testable only near an avatar
  wearing an alpha layer / mesh body (aditi), so the deterministic classification
  is the guarantee.
- [x] **P14.4. Refresh on rebake.** Re-request bakes on `RebakeAvatarTextures`
  and on a newer `cof_version` in a later `AvatarAppearance`.

  **Done.** Two refresh triggers were wired up. (1) *Our own avatar,
  `RebakeAvatarTextures`:* `appearance.rs`'s `drive_server_bake` now tracks
  whether the central-baking `UpdateAvatarAppearance` capability was ever
  offered (`ServerBakeState.cap_available`), and on an
  `Event::RebakeAvatarTextures` — the simulator telling us it lost one of our
  baked textures — re-runs the one-shot server-bake handshake from `Done`
  (re-query the COF version → re-POST the bake) so the grid re-composites and
  re-publishes our appearance. A rebake arriving mid-handshake is ignored (the
  in-flight bake satisfies it), and without the capability (OpenSim) it is
  inert. (2) *Any avatar, newer `cof_version`:* `ingest_avatar_bakes` re-fetched
  on every `AvatarAppearance` already; it now gates on the COF version
  (`AvatarState.baked_cof_version` + `should_refetch_bakes`) so a later
  appearance whose `cof_version` is *strictly older* — an out-of-order /
  duplicate resend — is skipped and cannot clobber a newer bake, while a newer
  *or equal* version still re-fetches (equal covers a same-outfit rebake
  republishing new baked ids at the same version) and an appearance with no
  `cof_version` (OpenSim / the older path) always ingests. Unit-tested
  (`should_refetch_bakes` cases); no library-surface change (viewer-internal —
  the `RebakeAvatarTextures` event and `cof_version` field already existed and
  are re-exported wholesale). The triggers are sim-initiated / outfit-change
  driven and cannot be forced deterministically, so the unit-tested gate is the
  guarantee, as with P14.3.
- [x] **P14.5. Clothing-morph alpha masks.** The second half of the original
  P13.5, split out here because it needs the baked-texture alpha pipeline built
  in P14.1–P14.3. Firestorm `LLPolyMorphTarget::applyMask` /
  `mIsClothingMorph`: the flared sleeve / pant-leg / long-cuff / loose-body
  geometry is driven by `clothing_morph="true"` params (`Shirtsleeve_flair`,
  `Leg_Pantflair`, `Leg_Longcuffs`, `Displace_Loose_Upper/Lowerbody`, the
  `skirt_*` morphs) whose `<mask layer="upper_clothes/lower_pants/skirt">`
  associates them with a clothing layer. In the reference viewer the per-vertex
  `maskWeight` fed into the morph (and the resulting clothing alpha) comes from
  the **baked texture's alpha channel** (`onBakedTextureMasksLoaded` sampling
  the baked upper/lower/skirt image) — so it can only land once the baked
  textures
  are fetched and decoded (P14). Apply that per-vertex clothing alpha through
  the base-mesh shared-vertex remap table (`SharedVertex`, already decoded) and
  render those vertices with `AlphaMode::Blend` / `Mask`, so an un-clothed body
  shows no stray flared cuffs.

  **Done — realised as a per-vertex *geometry* mask, not an alpha render.** The
  reference mechanism (`LLPolyVertexMask::generateMask` +
  `LLPolyMorphTarget::applyMask`) does not draw the clothing morph with a
  transparent alpha; it scales each clothing morph's per-vertex position/normal
  delta by the baked-region alpha sampled at that vertex's UV, so the flare
  geometry itself vanishes where there is no fabric — that is what "no stray
  flared cuffs" needs, and what shipped. The `<mask layer="skirt">` case from
  the roadmap text does not exist in `avatar_lad.xml` (its `<morph_masks>` table
  has seven entries, all `head` / `upper_body` / `lower_body`), so no skirt
  morph is masked. **Library (`sl-avatar`):** a new `masks` module —
  `MorphMasks::from_xml` parses the `<morph_masks>` table (`morph_name` /
  `body_region` / `layer` / `invert`); `MaskTexture` samples a decoded bake's
  alpha (nearest + clamp, last-component, mirroring `generateMask`);
  `MorphMasks::sample_part` walks a base part's masked morphs, sampling each
  delta vertex's UV through the shared-vertex remap into a `PartMorphMask` of
  per-delta weights; and `MorphWeights::apply_masked` (a thin variant of
  `apply`) scales each masked delta by `weight * maskWeight`. All re-exported
  through `sl-client-bevy`. **Viewer:** `AvatarAssetLibrary` also parses
  `MorphMasks` from the one `avatar_lad.xml` read;
  `BodyRegion::morph_mask_region` maps the head / upper / lower regions to their
  `<morph_masks>` names; `apply_avatar_appearance` now masks each masked part's
  morphs by its region's decoded bake (`part_clothing_mask`) and re-shapes the
  body when a masked-region bake decodes (a second `TextureDecoded` reader
  re-dirties the wearing avatar) — so the morphs apply at full flare until the
  bake arrives, then snap to the masked shape, exactly as the reference viewer
  does before/after `onBakedTextureMasksLoaded`. Unit-tested end-to-end (mask
  parse, nearest-sample, `sample_part` full/zero-alpha, `apply_masked`
  per-vertex scaling, region↔slot mapping). Like P14.3/P14.4 the trigger (a
  decoded clothing bake carrying a coverage-alpha channel) is outfit-driven and
  cannot be forced deterministically, so the unit-tested Firestorm-faithful path
  is the guarantee; it is exercised live only near an avatar wearing flared
  system-layer clothing.

## Phase 15 — Client-side baking (`sl-bake`, the OpenSim/legacy path)

The server-published path (Phase 14) covers *other* avatars on both grids, and
our *own* avatar on SL. It does **not** cover our own avatar on OpenSim (and any
grid without server bake): those grids expect the *client* to composite the bake
from wearable layers (legacy `UploadBakedTexture`). Without it our own avatar is
an untextured cloud. This phase composites the bake ourselves, primarily for our
own avatar and as the fallback whenever a baked slot is absent / default.

- [x] **P15.1. Scaffold `sl-bake` + region compositing.** New pure crate
  (scaffold like P12.1; `sl-texture` dep with `default-features = false`). Given
  the ordered per-region layers (skin → tattoo → clothing → alpha mask) as
  decoded `DecodedImage`s + their params (tint colour, alpha, tex-gen),
  composite each bake region (head/upper/lower/eyes/skirt/hair) into a baked
  RGBA. Alpha layers carve the alpha channel. Tests over synthetic layers.
  `cargo test -p sl-bake`. Done: `BakeRegion` (`region.rs`, mapped to the
  `sl_proto::avatar_texture` baked slots) plus a `composite.rs` layer engine —
  `Layer` (`LayerKind` Base/Blend/AlphaMask + tint/opacity/`TexGen`/invert
  builders, optional image for a solid fill) and `composite_region`, which walks
  the stack over a transparent canvas (base writes all channels, blend is
  source-over, alpha-mask carves dest alpha — grey masks read via luminance,
  4-component masks via their alpha), bilinearly resampling each layer to the
  bake size. `BakedImage::to_decoded_image` feeds the composite into the
  texture-consuming paths for P15.3. 17 unit tests over synthetic layers.
- [x] **P15.2. Wearable layer inputs.** Read the agent's worn wearables
  (`AgentWearables` / the COF), fetch each wearable **asset** (skin / tattoo /
  clothing / alpha) to get its layer texture ids + tint (which visual params
  colour a layer, e.g. skin tone), and decode the layer textures through the
  shared `TextureManager`. Assemble the per-region layer lists `sl-bake` needs.
  Done: `sl-proto` gained the per-wearable `TextureEntry` layer-slot constants +
  a `LAYER_TEXTURES` name/wearable-type table; `sl-avatar` a `WearableAsset`
  parser (the `LLWearable` text format) and a `bakecolor` tint evaluator
  (`ColorRamp`/`ColorOp` + `LLTexGlobalColor`/`LLTexParamColor`
  `calculateTexLayerColor`, keyed to the three `<global_color>`s); `sl-bake` a
  `plan` module — the ordered worn-wearable layers per region (from
  `avatar_lad.xml`'s `<layer_set>`) and `region_layers`, which resolves each
  planned layer's texture + tint into the compositor's `Layer` list. The
  viewer's new `bake_inputs` module drives our own avatar: `RequestWearables` →
  fetch each wearable asset over `ViewerAsset` (a `WearableAssetManager`
  mirroring the texture/mesh managers) → parse → request its layer textures →
  assemble the per-region lists into an `OwnBakeInputs` resource. Live on
  OpenSim the default outfit assembles
  `head=2 upper=3 lower=3 eyes=1 skirt=0 hair=1`.
  **Scope note:** only worn-wearable *texture* layers (skin bodypaint, clothing,
  tattoos, alpha masks) plus the solid skin-tone base are modelled — the
  reference viewer's procedural cosmetic param-layers (skin shading, make-up,
  freckles, bump maps) need a per-param procedural renderer the P15.1 compositor
  does not have and are left to a follow-up. Rendering these inputs onto the
  body is P15.3.
- [x] **P15.3. Composite & render our own bake.** When no server bake is
  published for an avatar (our own on OpenSim), composite its regions with
  `sl-bake` and drive the Phase-14 body-region materials + Phase-17 BoM from the
  local composite instead of a fetched baked UUID (alpha honoured). Verify our
  own avatar renders skin/clothing-textured on OpenSim. **Done (Phase-14 body
  regions; the Phase-17 BoM half is deferred with Phase 17):** a new
  `OwnLocalBake` resource + `apply_own_local_bake` system (`avatars.rs`)
  composites each ready `OwnBakeInputs` region (P15.2) through
  `composite_region` at 512², uploads it, and drapes it onto our own avatar's
  body-region materials for every slot the grid did **not** server-bake —
  reusing the P14 per-`(agent, slot)` region material so a real server bake
  (Second Life) still wins, and self-healing after
  `assign_avatar_bake_materials` resets a part. A region with no worn layers is
  skipped (an empty composite is fully transparent and would wrongly carve the
  region). Two live-found orientation/alpha fixes were needed on top of the
  plan: (a) Second Life avatar `.llm` UVs are OpenGL bottom-up, so the
  composited bake (top-down, like every decoded J2C) is flipped vertically
  before upload (`flip_rows_vertically`), else the head bake reads
  upside down (chin/teeth on the forehead); (b) the eyeball is opaque geometry
  but our simplified eye composite carries only the iris layer (not the opaque
  sclera base the reference eye layer-set builds), whose transparent surround
  classified the bake `Masked` and carved the eyeballs into empty sockets — so
  the eyes region bake is forced opaque (`force_alpha_opaque`). Verified live on
  OpenSim: our own avatar renders skin/clothing-textured, right-way-up, with
  visible eyeballs (default outfit composites `head`/`upper`/`lower` opaque +
  `eyes` forced-opaque + `hair` masked; `skirt` empty). The eyeball vertical
  placement issue this surfaced is tracked separately as P15.5.
- [x] **P15.4. (Optional) Publish the bake.** J2C-**encode** the composited
  regions and upload via the existing `UploadBakedTexture` cap so the sim /
  other viewers see us. **Needs a J2C encoder** (OpenJPEG encode) — the one
  heavy net-new dependency; may slip to a follow-up. Local rendering (P15.3)
  does not depend on it. **Done (verified live on OpenSim):** the encoder is a
  new `sl-j2c-encode` crate — an in-memory OpenJPEG-C (`openjpeg-sys`, the same
  backend `jpeg2k` decodes with) encode of RGBA8 → raw `.j2c` (opaque regions
  written RGB, transparency kept as a
  fourth component so an alpha-masked bake round-trips), isolated as the only
  `unsafe`-FFI crate in the workspace and surfaced through `sl-texture`'s new
  `encode` feature as `encode_j2c(&DecodedImage)` (encode→decode round-trip
  tested). The viewer's new `bake_publish` module (`OwnBakePublish` +
  `drive_bake_publish`) is a one-shot gated on the region advertising
  `UploadBakedTexture` (so it is naturally OpenSim-only — Second Life bakes
  centrally and never advertises it): once the P15.2 inputs are ready it
  composites each region (`composite_own_region`, factored out of
  `build_local_bake` so the exact same canonical bytes are draped *and*
  uploaded), J2C-encodes it, and uploads the regions **one at a time** (the
  `AssetUploaded` reply carries no correlation id, so uploads are serialised
  and spread one encode per frame), then advertises the uploaded baked-texture
  ids in an `AgentSetAppearance` (`Command::SetAppearance`) so the sim
  broadcasts our textured avatar. `CAP_UPLOAD_BAKED_TEXTURE` was promoted to a
  public re-export in `sl-client-bevy` (mirroring `CAP_VIEWER_ASSET`). Live on
  OpenSim the default outfit uploaded 5 regions
  (head/upper/lower/eyes/hair; skirt empty) — the sim accepted every encoded
  codestream and returned a fresh asset id per region, and the appearance
  published, with the P15.3 local drape unchanged. **Orientation:** the
  uploaded bytes are the vertically-flipped composite (the canonical bottom-up
  bake orientation SL server bakes are stored in, which is why the P14
  fetched-bake drape renders straight), so a real bake and our own upload
  agree. **Scope:** the publish carries a *neutral* visual-parameter set —
  P15.4 delivers the bake **textures**; publishing the worn **shape** needs
  the deferred high-level appearance API (a Phase-14 follow-up note). Verifying
  *other* viewers see the result needs a second observer and was not done here;
  the sim accepting each upload + the publish is the guarantee.

## Phase 16 — Attachments (rigid)

- [x] **P16.1. Detect & parent.** In `objects.rs` `reconcile_parent`, when an
  object's `parent_id` resolves to a **pcode-47 avatar** (not a prim linkset),
  decode `attachment_point()`, look up that avatar's skeleton **joint entity**
  (Phase 13), and parent the attachment there via `ChildOf` so it follows the
  posed skeleton. Hold-pending when the avatar/joint is not present yet (reuse
  the existing pending-adoption path). **Done:** `apply_object` marks an object
  whose `attachment_point_id()` is set as an attachment (its `parent` is the
  avatar) and holds it parentless rather than reconciling a linkset root; a
  companion `adopt_pending_attachments` system (the pending-adoption pattern,
  in its own system because the avatar's skeleton lives in `AvatarState` /
  `AvatarBody`, resources `update_objects` cannot reach and which are populated
  by a later system) resolves each pending attachment's target joint —
  raw point id → skeleton joint index (`AvatarBody::attachment_joint_index`,
  from the `avatar_lad.xml` `<attachment_point>` table now parsed into
  `AvatarAssetLibrary`) → the avatar's joint entity
  (`AvatarState::attachment_joint_entity`, from a new per-agent joint-entity
  store) — and `ChildOf`-parents it, retrying on later frames until the
  avatar/joint exists. A sphere-only (no `--viewer-assets`) avatar has no
  skeleton, so its attachments fall back to the avatar object entity (position
  only), preserving the pre-P16 behaviour. **Synthetic `mRoot`:** the reference
  viewer creates an `mRoot` joint above `mPelvis` in code (it is not in
  `avatar_skeleton.xml`), so the avatar-centre attachment point
  (`joint="mRoot"`) had no joint to resolve to;
  `BevySkeleton::insert_synthetic_root`
  appends an identity root above the former roots (indices unchanged), which the
  viewer adds after building the skeleton — with it all 47 non-HUD attachment
  points resolve to a real joint (8 HUD points, whose `mScreen` is not a body
  joint, stay unresolved for Phase 19). Verified live on OpenSim: assets load
  (134 joints incl. `mRoot`, 55 attachment points) and the rigged avatar shapes
  cleanly across 134 joints with no panic from the new systems; the
  attachment-*tracks-the-avatar* live check (needs a worn attachment) is
  P16.2's.
- [x] **P16.2. Attachment transform.** Place the attachment at its stored local
  offset/rotation relative to the joint; honour attachment `ADD_FLAG` vs
  replace. **Done:** the reference viewer models each attachment point as a
  node parented to its skeleton joint at the fixed `avatar_lad.xml`
  `position` / `rotation` offset (`LLViewerJointAttachment`), with the worn
  object's own local transform relative to *that node* — not the bare joint. So
  P16.1's direct joint-parenting seated an attachment at the joint origin,
  missing the point offset (e.g. the Chest point sits `0.15 0 -0.1`, rotated
  `0 90 90`, off `mChest`). `AttachmentPointInfo` now carries each point's
  offset (`avatar_assets.rs`), `AvatarBody` resolves it into a
  `BodyAttachmentPoint { joint_index, offset: Transform }`, and `spawn_body`
  spawns one **attachment-point node** entity per point as a child of its joint
  carrying that offset (a new per-agent `AvatarState::attachment_nodes` store,
  despawned with the body). `adopt_pending_attachments` now parents a worn
  attachment to the node (`attachment_point_entity`) instead of the joint, so
  the object's existing child transform (local pos/rot in Second Life Z-up)
  composes onto the point offset — the full joint → point → object chain. The
  offset is kept in the joint's Second Life Z-up frame (no basis change), like a
  linkset child's local transform; a new `coords::sl_euler_deg_to_quat`
  reproduces `LLQuaternion::setQuat(roll, pitch, yaw)` verbatim so the point
  rotation matches the reference viewer exactly (unit-tested vs the glam
  single-axis quaternions). **`ADD_FLAG`:** nothing to honour on the render
  side — the transient `ATTACHMENT_ADD` (`0x80`) bit is already stripped in
  `sl-proto`'s `attachment_point_from_state`, and add-vs-replace is a
  server-side inventory concern (a replaced attachment is removed by
  `KillObject`, handled via `ObjectRemoved`); the viewer simply renders every
  attachment the server streams on its point. **Verified live on OpenSim:** a
  cube worn at the Chest point (local pos `0,0,0`, so it seats exactly at the
  chest node's offset from `mChest`) on one avatar is seen by a second observer
  avatar's viewer, which spawns both rigged bodies and logs `parented
  attachment … (point 1) to avatar … joint` with no panic from the new
  node-spawning path.

## Phase 17 — Rigged mesh & bake-on-mesh

- [x] **P17.1. Skinning math.** In `sl-avatar` `skin.rs`: a matrix-palette
  helper taking `sl_mesh::MeshSkin` (joint names + inverse-bind + bind-shape +
  alt-bind + `pelvis_offset` + `lock_scale_if_joint_position`) and per-vertex
  `VertexWeights` against a `Skeleton` instance's current joint world transforms
  → skinned vertices (≤4 weights). Tests with a synthetic skeleton.
  **Shape:** `SkinningPalette::build(&skin, |name| Option<world_matrix>)` folds
  each rig joint into `inverse_bind_matrix[j] * joint_world_matrix[j]`;
  `skin_position` / `skin_normal` then apply `v * bind_shape` and the
  weight-normalized blend of the palette matrices (mirroring Firestorm's
  `initSkinningMatrixPalette` + `getPerVertexSkinMatrix` +
  `updateRiggedVolume`). All matrices are SL's row-vector row-major `[f32; 16]`
  (same layout `sl-mesh` decodes), so this stays Bevy-free and glam-free — a
  hand-rolled `[f32; 16]` mat-mul / affine transform under the crate's strict
  lints. The joint world transforms are an **input**: the caller (P17.2) poses
  the skeleton instance, and `alt_inverse_bind` / `pelvis_offset` /
  `lock_scale_if_joint_position` are honoured there (they shape the world
  matrices), not in the palette algebra. Missing-joint fallback matches the
  reference viewer (world = identity → palette entry is the bare inverse-bind).
  10 unit tests over a synthetic skeleton (translation/blend/normalization,
  inverse-bind↔world cancellation, bind-shape ordering, missing/out-of-range
  influences, normal rotation without translation). New `sl-avatar → sl-mesh`
  dependency for `MeshSkin` / `VertexWeights`.
- [x] **P17.2. Rigged-mesh rendering.** A mesh object with a skin block worn on
  an avatar renders as a Bevy `SkinnedMesh` bound to that avatar's skeleton
  instance (not a static child), so mesh bodies/clothing deform with the avatar.
  Reuse the `MeshManager` fetch/decode; join to the avatar via the Phase-16
  attachment association. **Shape:** `MeshManager` now decodes the skin block
  alongside geometry; `apply_object_meshes` diverts a *worn* rigged mesh
  (attachment + skin) to a deferred `PendingGeometry::RiggedMesh`, and a new
  `apply_rigged_attachments` system binds it once the wearer's skeleton instance
  exists — spawning one `SkinnedMesh` submesh under the avatar body root, joints
  resolved from the skin's `joint_names` (unknown → pelvis fallback, logged).
  `to_bevy_rigged_mesh` / `rigged_inverse_bindposes` (in `sl-client-bevy`) build
  the `JOINT_INDEX`/`JOINT_WEIGHT` attributes and fold the bind-shape into each
  inverse bindpose (row-major `[f32;16]` → `Mat4::from_cols_array` is the needed
  transpose). **Crucial live finding:** mesh bodies/clothing rig heavily to the
  avatar's **collision volumes** (`PELVIS`, `BELLY`, `L_UPPER_ARM`, …), not just
  bones — so `BevySkeleton::from_skeleton` now appends each bone's collision
  volumes as extra joints (parented to their bone at the `avatar_skeleton.xml`
  pos/rot/**scale**, matching the reference viewer's `setupBone`); without them
  every collision-volume weight fell back to the pelvis and the mesh ballooned
  into a sphere. Verified live on aditi (a worn mesh body + clothing binds and
  deforms correctly; the body's own **skin** stays untextured until P17.3).
- [x] **P17.3. Bake-on-mesh.** A worn rigged (BoM) body face whose
  `TextureEntry` slot is an `IMG_USE_BAKED_*` sentinel is textured from the
  wearer's own baked avatar texture rather than fetched. **Shape:** a
  `BomFace` marker (agent + baked slot) tags such faces in
  `build_rigged_submeshes` (spawned with the opaque body-skin placeholder,
  never the sentinel — the P17.2 invisible-shell finding);
  `apply_bom_face_materials` then mirrors each face onto its wearer's
  matching base-region material every frame, so it follows whichever bake
  resolved that region (server bake on SL, client composite on OpenSim) and
  its alpha, updating in place as the bake decodes. The `IMG_USE_BAKED_*`
  constants already existed from P16's region-hide.
  **Three cross-cutting fixes were needed to render a real SL mesh body:**
  (1) **P17.2 binding bug** — a mesh body is worn as a multi-prim *linkset*
  whose rigged parts parent to the linkset **root prim**, not the avatar, so
  the old `body_root(tracked.parent)` never resolved (146k "skeleton not
  ready" retries → invisible body); `apply_rigged_attachments` now chases
  the parent chain to the wearer (`AvatarState::wearer_of` →
  `avatar_root_of`). (2) **Server-bake fetch** — a SL server ("Sunshine")
  bake is *not* fetchable by UUID from the `GetTexture`/`ViewerAsset` CDN
  (it 503s); it lives on a separate **appearance service** whose base URL
  arrives in the `agent_appearance_service` login field. Added: parse it in
  `sl-wire` `LoginSuccess` → expose on `Session` → deliver as
  `SlIdentity::agent_appearance_service`; a typed `sl-texture`
  `TextureFetchType` (full, mirrors the reference `FTType`) narrowed to a
  remote-only `RemoteTextureSource` via `TryFrom` (local-generated kinds —
  media-on-a-prim, local files — error at that boundary before the store)
  threaded through `TextureStore::get`/`request` and both runtime fetchers,
  which pick the CDN (by UUID) or the bake's URL
  (`<svc>texture/<avatar>/<slot>/<uuid>`); the bake is stored/decoded in the
  normal store keyed by its UUID. (3) **5-component J2C** — a server bake is
  a 5-component codestream (`R, G, B, bump, clothing`), which `jpeg2k`'s
  `get_pixels` rejects; `decode_j2c` reads the diffuse RGB from the first
  three components (opaque alpha, dropping bump/clothing), matching the
  reference `decodeChannels(.., 0, 4)`. Also fixed the **mesh UV V-flip**
  (SL mesh UVs are OpenGL bottom-up, Bevy samples top-down) so clothing and
  the BoM body map correctly instead of near-uniform, and set a
  **0.02 m camera near plane**. Verified live on aditi: a BoM mesh body
  binds, deforms, and shows the wearer's server-baked skin +
  correctly-mapped clothing. Remaining avatar-fidelity bugs this surfaced
  (skinning distortion, rigid eyes/teeth, prim params) are collected under
  **Known rendering issues** below.

## Phase 18 — Animations (full pipeline)

- [x] **P18.1. Scaffold `sl-anim` + `.anim` decode.** New pure crate (scaffold
  like P12.1). Decode the Linden keyframe-motion binary → `Motion`
  with per-joint rotation/position keyframe tracks, priority, ease-in/out, loop
  points, and constraints. Fixture-based tests. `cargo test -p sl-anim`.
  **Done:** the decoder lives in `decode.rs` (named for its role and to avoid
  the `module_name_repetitions` lint on `motion::Motion`, mirroring
  `sl-mesh`/`sl-texture`'s `decode` module) and is re-exported at the crate
  root. `Motion::from_bytes(&[u8])` decodes the whole file: the header
  (`base_priority`, `duration`, `emote_name`, loop points, ease-in/out,
  `hand_pose`), the per-joint tracks, and the collision-volume `Constraint`s,
  applying the reference viewer's range/finiteness validations (bad priority,
  over-long/`NaN` duration, too many joints, negative key counts, out-of-range
  key time, unknown constraint type/over-long chain → a typed `AnimDecodeError`;
  a corrupt constraint *count* is skipped, not fatal, matching the reference).
  Quantised values are widened exactly like the C++ (`U16_to_F32` with its
  near-zero snap; rotations completed to a unit quaternion via
  `unpackFromVector3`). **Both** wire versions decode: the modern `1.0`
  (`u16`-quantised) form and the legacy `0.1` form (`f32` times, `f32` Euler
  angles built with a `mayaQ`/`ZYX` port, `f32` positions clamped to `[-5, 5]`)
  — the latter still backs many decades-old SL animation assets that visual
  updates never replace. Priorities/hand poses are newtypes (`JointPriority` /
  `HandPose`) with named constants; constraint kind/target are enums. A
  forward-only `Cursor` reads little-endian primitives via `f32::from_bits` /
  byte-fold shifts / `u32::cast_signed` (the crate lints forbid `from_le_bytes`,
  `as`, indexing, `unwrap`/`expect`/`panic`). Two committed binary fixtures
  (`tests/fixtures/minimal.anim` v1.0, `minimal_old.anim` v0.1) drive eight
  round-trip + error-path tests.
- [x] **P18.2. Built-in animation library.** Resolve an `anim_id` to its asset:
  built-in fixed-UUID motions from the `--viewer-assets` path, else fetch an
  uploaded `.anim` over `ViewerAsset` (reuse the asset fetch path). Cache
  decoded motions. **Done:** a new `sl_anim::registry` module (named for its
  role, like `decode`, to dodge `module_name_repetitions`) ports the reference
  viewer's 140 `ANIM_AGENT_*` built-in UUIDs
  (`llcharacter/llanimationstates.cpp`), each tagged `BuiltinKind::Keyframe` (a
  downloadable `.anim` asset) or `Procedural` (the 48 walk/stand/turn/`LLEmote`/
  always-on-adjuster motions the reference viewer synthesises in C++ and never
  fetches — taken from `llvoavatar.cpp`'s `registerMotion` block), with a
  `builtin_animation(uuid)` lookup and six unit tests. The viewer's new
  `animations.rs` owns an `AnimationManager` resource driving the same
  `ViewerAsset` generic-asset store the P15.2 wearable fetch uses:
  `request(id)` skips a nil/cached/in-flight/known-unavailable id, records a
  procedural built-in as unavailable *without* a fetch (fetching its UUID would
  404), and otherwise resolves the `.anim` bytes — first from a `<uuid>.anim`
  file under `--viewer-assets` (a pre-provisioned built-in; stock viewers ship
  none, so this is the escape hatch and downloadable built-ins arrive over
  `ViewerAsset` like uploads), else over `ViewerAsset` — decoding to a `Motion`
  off the render thread on the `IoTaskPool` and caching it by UUID (shared
  across every avatar playing it). `ingest_avatar_animations` requests a motion
  for every animation each `AvatarAnimation` lists; `poll_animations` folds a
  finished decode into the cache and announces `AnimationDecoded`. The
  `motion()` accessor + the event carry the P18.3 seam (`#[expect(dead_code)]`
  until then). Verified live on OpenSim with the real skeleton loaded (Firestorm
  `character/` dir via `SL_VIEWER_ASSETS`): the agent's own `stand` is ingested,
  resolved against the registry, and correctly classified procedural / not
  fetched. The download+decode branch was not triggered live — an idle OpenSim
  avatar only ever signals the procedural `stand` — but it is covered by
  `sl-anim`'s decode unit tests and reuses the P15.2 `ViewerAsset` fetch path
  already proven on OpenSim. No visible avatar motion yet: posing the skeleton
  from the cached motions is P18.3.
- [x] **P18.3. Drive the skeleton.** On `Event::AvatarAnimation`, for each
  `PlayingAnimation` sample its `Motion` each frame and pose the target avatar's
  skeleton-instance joints (via a `sl-client-bevy` animation driver / Bevy
  clip). Attachments (Phase 16) and rigged mesh (Phase 17) follow automatically.
  Verify a walking/waving avatar. **Done.** Pure sampling lives in a new
  `sl-anim` `sample` module (inherent `Motion` / `JointMotion` methods,
  Bevy-free): `Motion::playback_time` maps elapsed seconds to the time within
  the motion honouring loop points (mirrors `LLKeyframeMotion::onUpdate`),
  `is_expired` retires a finished one-shot, and
  `JointMotion::sample_rotation` / `sample_position` interpolate the keyframe
  curves (the reference viewer's `RotationCurve` / `PositionCurve` `getValue` +
  `nlerp`, so `.anim` rotations widen to unit quaternions). `sl-client-bevy`
  gains a `sample_motion(&Motion, elapsed) -> Vec<SampledJoint>` adapter (SL
  Z-up `Quat` / `Vec3`, the animation mirror of `to_bevy_*`). The viewer's
  `animations.rs` grew the driver: `drive_avatar_skeletons` (Update) folds each
  `AvatarAnimation` set into a playback clock (a fresh `sequence_id` restarts a
  motion) and resolves a per-joint `AnimationPose` (highest joint priority wins
  across concurrent motions — full ease / blend is P18.4), and
  `pose_avatar_skeletons` (PostUpdate, after transform propagation) writes each
  rigged avatar's joint **world matrices** straight into their
  `GlobalTransform`s. Verified live on OpenSim: the agent's own avatar plays a
  built-in `.anim` (a new `--play-animation <uuid>` debug flag drives the own
  avatar via `Command::PlayAnimation`, added on user request to exercise the
  driver from a single login), fetched over `ViewerAsset` from OpenSim's
  library asset set, decoded off-thread (dance1 = 19 joint tracks / clap = 10),
  and the skeleton posed and returned to rest. Three fixes fell out of live
  testing, all in the render crates: (1) the driver writes joint globals
  **directly** rather than overlaying the keyframe rotation onto the
  baked-scale rest `Transform` (a local `T·R·S` shears a non-uniformly-scaled
  joint under rotation) — `BevySkeleton` gained `deformed_world_matrices(deform,
  overrides, pose)`, the SL skeletal recurrence with the animation pose folded
  in, and an `AnimationPose` type; (2) a position track (`mPelvis`) is a
  **relative** offset *added* to the rest position, not an absolute one that
  would collapse the pelvis ~1 m to its parent origin; (3) every rigged
  avatar's globals are rewritten **each frame** (its animated pose or its plain
  deformed rest) so an avatar un-freezes to rest when its motions stop and
  several overlapping motions with different runtimes compose — Bevy's
  dirty-bit propagation cannot recompute a static joint whose global the driver
  overwrote. **Known limitation (deferred, R11):** the base body's mesh still
  shows limb distortion under animation because it is skinned with standard
  inverse-bindpose LBS, not Second Life's `LLSkinJoint` pivot scheme; the
  skeleton itself is posed correctly (bone lengths stay constant — verified
  live), so this is a P13.1 skinning-fidelity gap, invisible at rest and
  orthogonal to the driver.
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

## Known rendering issues (to fix)

Avatar / prim rendering-fidelity bugs, several surfaced by the live BoM-avatar
review on aditi in P17.3. These are **pre-existing** gaps separate from the
feature phases above; each needs iterative visual debugging against a live
avatar, so they are collected here to be worked one at a time.

- [x] **R1. Rigged-mesh skinning distortion.** Two independent fixes, the first
  being the actual cause of the visible distortion (confirmed live: pants, feet,
  and the mesh-head teeth / eyes / eyelids all render cleanly after it).
  - **Un-normalized skin weights (the real fix).** A worn rigged mesh's
    per-vertex weights were fed to Bevy raw. Second Life stores each influence
    as an independent quantized fraction and drops influences past the fourth,
    so a vertex's weights need not sum to one — but Bevy's skinning shader
    (unlike the reference viewer's `getPerVertexSkinMatrix`) does **not**
    renormalize, so a weight sum `s < 1` blends in `(1 - s)` of the zero matrix
    and drags the vertex a fraction of the way to the mesh origin — the downward
    "streak toward the feet" of a rigged garment / head part. Fixed by
    renormalizing the four weights to sum to one in `pack_influences`
    (`sl-client-bevy` `meshes.rs`); a zero-sum vertex binds fully to slot 0.
    This is what fixed the pants / feet and, as a bonus, the BoM-head teeth /
    eyes / eyelids (also worn rigged mesh). The base system body was never
    affected — it uses the (already normalized) adjacent-joint blend path.
  - **Joint position overrides (fitted-body proportions / fingers).** A fitted
    mesh body/head also ships an `alt_inverse_bind_matrix` per joint (the upload
    "include joint positions" option) that repositions the skeleton to the pose
    its inverse-binds assume; a worn rigged mesh carries its **own**
    inverse-binds, so without the overrides its extremities sit slightly off
    (the base body self-cancels, being skinned against *our own* bindposes).
    Implemented as the reference viewer's `addAttachmentOverridesForObject`:
    `joint_position_overrides` / `JointOverrides` +
    `BevySkeleton::deformed_local_transforms_with` (0.1 mm threshold, replaces
    the joint's local rest position, honours `lock_scale_if_joint_position`),
    stored per contributing mesh so a per-joint conflict resolves to the highest
    mesh id (`findActiveOverride`) and the set rebuilds as meshes come and go
    (`clearAttachmentOverrides`). **Animesh (animated objects) are excluded** —
    they drive their own control-avatar skeleton (`!vo->isAnimatedObject()`),
    detected via the linkset root's `ExtendedMesh` `ANIMATED_MESH_ENABLED` flag;
    without this a giant / rotated-frame animesh worn nearby would catapult the
    wearer's skeleton. On the test avatar its own body's overrides are ≈0, so
    this part is a near-no-op there; it targets bodies that genuinely reposition
    joints. Toggle `SL_VIEWER_JOINT_OVERRIDES=0` disables it. `pelvis_offset` is
    left unapplied (a hover/height concern, not distortion; `0.0` on every
    observed body).
- [x] **R2. Fix rigid eyeball placement (was P15.5).** The rigid eyeballs read
  too low / recessed in the socket (a see-through gap above the eyeball). The
  perception-vs-measurement gap was **real**, with two independent causes, both
  now fixed (confirmed live on OpenSim — the eyes seat cleanly with white sclera
  and visible irises):
  - **Base-mesh skinning joint mapping (the actual placement cause).** Second
    Life base parts store one weight float per vertex whose integer part indexes
    the reference viewer's **`mJointRenderData`** list — a depth-first skeleton
    walk with each group's base ancestor prepended
    (`LLAvatarJointMesh::setupJoint`; `avatarSkinV.glsl`:
    `mix(palette[floor(w)], palette[floor(w)+1], fract(w))`) — **not** the
    mesh's own `joint_names` table. Our decoder mapped it into `joint_names` and
    clamped, so the head's `[mHead, mNeck]` names sent every face vertex (weight
    `2.0`) to `mNeck` instead of `mHead`. It renders correct at rest (the
    inverse bind-pose cancels it) but under skeletal deformation the whole face
    was dragged by the
    *neck* while the rigid eyeball (correctly on `mEyeLeft` → `mHead`) was not —
    the divergence. Fixed by keeping the raw weight index (`sl-avatar`
    `split_weight`) and rebuilding the render list (`sl-client-bevy`
    `base_mesh_skin` / `joint_render_data`). Also corrects the whole base body's
    shape under deformation, not just the eye.
  - **Missing eye sclera (the "untextured" half).** Our client-side eye bake
    carried only the iris layer, so the eyeball read as a featureless blob
    (easily misread as misplaced). Added the reference `eyes` layer-set's white
    sclera base (`eyewhite.tga`) under the iris — part of the broader static-TGA
    bake layers below.
  Note: the *rigid* eyeball itself has **no** placement offset in Firestorm
  (`setMesh` uses the `.llm` origin, pinned to `mEyeLeft`; eye tracking is
  rotation-only) — the fix was upstream, in the skeleton/skinning.
- [x] **R2b. Broader static-TGA bake layers.** The client-side bake modelled
  only worn-wearable texture layers + a solid skin-tone base; the reference
  bakes in static `character/` TGA diffuse layers on every avatar. Added a
  `LayerSource::Static` plan source (`sl-bake`) that loads/decodes the TGAs
  (`image` crate, viewer side) and composites them: the skin-grain base
  (`head_skingrain.tga` / `body_skingrain.tga`, tinted by skin colour, replacing
  the flat fill), the skin colour details (`head_color.tga` / `upperbody_color`
  / `lowerbody_color`), the eye sclera (`eyewhite.tga`), and the eyelash-shape
  alpha (`head_alpha.tga` — carves the lash surround out of the head bake so the
  eyelash mesh, which shares the head material, no longer renders an opaque
  quad). The procedural cosmetic / bump layers (shading, highlights, lipstick,
  blush, freckles) stay out — they need a per-param colour renderer. Eyelash
  rendering is only partly done: the opaque quad is gone, but the thin lashes
  need `AlphaMode::Blend` (they fall below the masked-bake cutoff) — folded into
  **R5**.
- [x] **R3. System eyes/teeth show through a BoM head.** Fixed by the R1
  **weight-normalization** fix (confirmed live: the mesh head's teeth, eyes, and
  eyelids now render cleanly). The "show through" was **misdiagnosed** as a
  hiding gap: those parts are the *worn mesh head's own* rigged eyes / eyelids /
  teeth, which had the R1 un-normalized-weight streak and protruded through the
  mesh face — not the system `avatar_head.llm` parts poking out. Renormalizing
  the skin weights seats them back inside the head. (The only remaining eye gap
  is a missing eye *texture*, a fetch/material matter, not geometry — out of
  scope here.) Note: this is distinct from **R2**, the *rigid* system eyeballs
  (`avatar_eye.llm`), which are unaffected by the skinning fix and stay open.
- [x] **R4. Prim rendering fidelity.** Two independent fixes; the "too large /
  misplaced / flat" perception was a real bug, distinct from the TE-placement
  gap. Live-verified against populated aditi builds (a crosshair pick tool,
  `pick_object` in `objects.rs`, press `P`, reports the object under the centre
  of the screen — full id, mesh/sculpt asset, scale, world-scale, shape — so a
  wrongly rendered object is identified by *looking* at it; plus a
  `SL_VIEWER_LOG_OBJECTS` diagnostic that flags region-sized / sky objects).
  - **Linkset children inherited the root's scale (the "too large / stretched"
    cause).** Every object entity carried `object.scale`, and a linkset child
    parents to the root entity — so Bevy composed `root_scale × child_scale`,
    oversizing children *and* shearing them (a non-uniform parent scale on a
    rotated child). Second Life prims each have an absolute size and never
    inherit the root's scale. Fixed by moving the scale off the object entity
    (now position/rotation only) onto a per-object **geometry holder** child
    ([`geometry_transform`]) that only that object's own faces hang off, so the
    scale reaches the geometry but never the child prims. Empty OpenSim has no
    linksets, so it never showed there.
  - **Per-face `TextureEntry` placement.** `scale_s` / `scale_t` (repeat),
    offset, and rotation are applied as the material's `uv_transform`
    (`texture_face_uv_transform` in `sl-client-bevy`, a port of the reference
    viewer's `llface.cpp` `xform` about the face centre), covering prim, sculpt,
    and mesh faces. Also fixed the **upside-down prim textures**: `sl-prim` UVs
    are OpenGL bottom-up, so `to_bevy_prim_mesh` now flips V (`1 - v`) to match
    `to_bevy_mesh` / wgpu's top-down sampling. (bump / shiny / glow / fullbright
    stay deferred — non-goals.)
- [x] **R5. Transparent-texture handling / alpha modes.** `face_material` no
  longer forces `AlphaMode::Opaque`: a face whose tint colour is non-opaque
  blends, and a face whose texture carries an alpha channel (2- or 4-component
  codestream) is upgraded to `AlphaMode::Blend` once it decodes — so the
  Second Life world's many transparent surfaces (invisible prims, glass, sky-
  platform floors) stop rendering as solid region-sized walls. Covers prim,
  sculpt, and mesh faces; finishes the **eyelashes** (from R2b), which now show
  with proper transparency. The precise legacy-materials `DiffuseAlphaMode`
  (mask cutoff / emissive) and avatar-face alpha stay deferred. Also: the
  all-`f` GLTF material-override null-texture sentinel
  (`GLTF_OVERRIDE_NULL_UUID`) is now treated as "no texture" rather than
  endlessly re-fetched (it 503s).
- [x] **R6. Avatar disappears when the camera zooms in close.** A Bevy skinned
  mesh's frustum bounds are its static bind-pose AABB placed at the mesh
  *entity's* transform, while the vertices render wherever the joint matrices
  put them — so the bounds need not match the drawn mesh even at rest, and the
  narrow near frustum of a close camera misses them, culling the avatar. Fixed
  with `NoFrustumCulling` on the avatar body parts and worn rigged meshes (so a
  close camera passes through the body as in Second Life). The near plane is
  unrelated (it can only clip front faces, not vanish the whole avatar; and a
  perspective near plane cannot be `0`).
- [x] **R7. Hollow / profile-cut prim tessellation (`sl-prim`).** A heavily
  hollowed, profile-cut cylinder (a curved "railing" wall) rendered see-through.
  The original diagnosis (inner wall / cut-end caps wound wrong) was
  **incorrect** — a winding analysis of the picked case (`profile_curve` circle,
  `profile_hollow` 0.95, cut 0.04–0.51) showed the outer wall (+radial), inner
  wall (−radial, faces into the hole), and both cut-end caps (`PROFILE_BEGIN` /
  `PROFILE_END`, facing the removed arc) were all wound outward correctly. The
  real culprit was the **path (top/bottom) caps**: `build_cap` always emitted a
  centre-vertex triangle **fan**, but a hollow prim's cap ring is
  `outer ++ reversed-inner`, so the inner-ring half of the fan wound backwards —
  ~half the cap triangles (measured: 37 `+Z` / 36 `−Z` on the top) were
  back-face culled, and you saw straight through the cap into the hollow
  interior (the "enclosed side vanishes"). Fixed by tessellating a **hollow cap
  as an annulus** (`build_hollow_cap` / `hollow_cap_indices` in `sl-prim`
  `volume.rs`), a faithful port of the reference viewer's `LLVolumeFace::
  createCap` hollow branch: an area-based ear split that walks one pointer
  forward from the outer-ring start and one backward from the inner-ring start,
  emitting the non-back-facing triangle at each step (top / bottom windings
  flipped) with no centre vertex — so the hole stays open and every triangle
  winds outward. A solid (non-hollow) cap keeps the centre fan. The
  `sl-client-bevy` `to_bevy_prim_mesh` bridge is unchanged (geometry-only).
  Regression test `hollow_cut_cylinder_caps_wind_consistently` asserts every
  path-cap triangle now winds `+Z` (top) / `−Z` (bottom) and that the cap is an
  annulus (tri count = vert count − 2, no centre fan).
- [x] **R8. Box-cap centre-fan cross (`sl-prim`).** Every plain box (cube)
  showed an **X / cross** through each cap face's texture. `build_cap` built
  the square cap as a centre-vertex **fan** (four triangles meeting at the
  middle), and a real texture reveals the fan's diagonals as a cross. The
  reference viewer never does this for a plain box — `createCap` routes a solid,
  uncut, full-path square-on-line prim to `createUnCutCubeCap`, a proper
  two-triangle quad grid (a `(grid_size + 1)^2` bilinear vertex grid, one quad
  per cell). Ported as `build_uncut_cube_cap` / `uncut_cube_indices`
  (+ `is_uncut_cube`) in `sl-prim` `volume.rs`, dispatched for that case; other
  solid caps (round / cut / tapered) keep the fan (the reference viewer fans
  those too, so they already match). Tests `box_caps_are_two_triangle_quads`
  (Lowest LOD: 4 verts / 2 tris / corner UVs) and
  `split_box_caps_are_a_consistent_grid` (High LOD: a square vertex grid, never
  a fan). **User-confirmed: cube cross gone.**
- [ ] **R9. Planar texgen, unconfirmed** (`TEX_GEN_PLANAR`).
  A flat, solid, uncut disk (a full cylinder) still looked wrong
  versus the reference viewer even though its cap is tessellated correctly
  (a fan with **exactly affine** UVs, which by the affine-interpolation property
  render the texture perfectly flat whatever the triangulation — proven, not a
  tessellation bug). The suspected cause is **texture-gen mode**: a face's
  `media_flags & 0x06` selects the UV source, and builders commonly set
  architectural prims to **planar** mapping (`TEX_GEN_PLANAR`, `0x02`). The
  reference viewer then ignores the volume's stored UVs and projects each
  vertex's texture coordinate from its position (scaled by the object size) and
  normal (`LLFace::planarProjection`); we always used the stored UVs. A
  candidate fix is implemented but **the live visual bug is not yet confirmed
  fixed**: `TextureFace::is_planar_texgen` (`sl-proto`), a `planar_texgen_uv`
  port (`sl-client-bevy`, unit-tested against hand-computed reference values),
  and `apply_planar_texgen` in the viewer — for a planar face it overwrites the
  built mesh's UV0 with the projection (positions × object scale, same `1 - v`
  flip as the stored UVs), keeping the per-face repeats/offset/rotation on the
  material's `uv_transform` afterwards (the reference viewer's
  `planarProjection` then `xform` order). Wired through prims, sculpts, and
  (unrigged) meshes.
  Worn **rigged** mesh attachments are not yet covered. **Open until verified in
  the running viewer against the reference viewer** — the fix may be incomplete
  or the real cause may differ.
- [x] **R10. Tiled faces need a repeating texture sampler.** The real cause of
  the half-cylinder / disk "streaked toward the edges, coherent in the centre"
  look (diagnosed from a live `pick` dump of the face's `TextureFace`: both
  faces were `planar=false`, so R9 was a red herring; the tell was the
  **repeats** — `scale_s`/`scale_t` of `(2, 1.6)` on the disk cap and `(10, 1)`
  on the railing wall). Repeats above one push the face UVs outside `[0, 1]` to
  tile the texture, but prim/mesh face images were uploaded with Bevy's default
  **clamp-to-edge** sampler, which smears the edge texel across every
  out-of-range tile instead of wrapping — heavy streaking at the rim, worse at
  higher repeats. Second Life samples object faces with a **repeat/wrap**
  address mode. Fixed in the viewer's `prim_image`: prim/mesh face textures now
  upload with a repeating sampler (`address_mode_u/v/w = Repeat`, linear
  filtering); avatar-bake and terrain paths are untouched. Also added a per-face
  texture-placement dump to the `pick` crosshair tool (`FaceTextureDebug`:
  repeats / offset / rotation / texgen / texture id) — the ground-truth
  diagnostic that found this. **User-confirmed:**
  the tiled faces now render "much closer to Firestorm". (A remaining colour /
  brightness difference is suspected to be lighting / tonemapping rather than
  texturing — a separate follow-up, not pursued here.)
- [ ] **R11. Base-body mesh distorts under animation** (`sl-avatar` /
  `sl-client-bevy`). Surfaced by P18.3: a *shaped* avatar's limbs (arms most
  visibly) stretch / distort while an animation plays, but look correct at rest
  and return to correct on stop. The **skeleton is posed correctly** — the joint
  world matrices are right and the bone lengths stay constant under animation
  (verified live from a per-frame `mShoulderLeft`→`mElbowLeft`→`mWristLeft`
  length dump: a steady `0.289` / `0.214` throughout dance1). The distortion is
  in the **skin**: the base body is skinned with standard inverse-bindpose
  linear-blend skinning (P13.1 `to_bevy_base_mesh` + `base_mesh_skin`), whereas
  the reference viewer skins the *system* avatar with the `LLSkinJoint`
  **pivot** scheme — `LLViewerJointMesh::uploadJointMatrices` bakes each joint's
  skin pivot (`mRootToJointSkinOffset` / `mRootToParentJointSkinOffset`) into
  the skinning matrix (`gJointMat.translate(pivot)`) before `updateGeometry`
  blends `jointMat[joint]` / `jointMat[joint+1]` per vertex. The two schemes
  agree when every joint's relative rotation is identity (rest) but diverge once
  a joint rotates, so the gap is invisible until animation. The fix is to
  reproduce the skin-pivot skinning for the base body (compute the per-joint
  skin offsets and fold them into the inverse bindposes, or drive skinning from
  pivot-adjusted joint matrices). Needs careful visual iteration against a live
  animated avatar; rigged-mesh bodies (Phase 17, ordinary skin weights) are
  unaffected.
- [ ] **R12. Own avatar renders bloated — publish/resolve the worn shape**
  (`sl-client-bevy-viewer`). Diagnosed by a Firestorm vs local-OpenSim
  side-by-side: our own avatar renders with a bloated body and vertices
  spiking out of the head/hair **at rest** (no animation), while Firestorm
  renders the same account as the correct slender shape. Root cause is the
  client-side bake publish (P15.4, `bake_publish.rs`): it advertises a
  placeholder **all-`128` "neutral" visual-parameter vector**
  (`neutral_visual_params`), but `128` is the range *midpoint*, and most
  body-shape morphs are **asymmetric** (default `0`, range `0..N`), so `128`
  is ~50% strength on every one → permanent bloat + displaced head/hair. The
  own avatar's shape is rendered from the server's
  `AvatarAppearance.visual_params` (`apply_avatar_appearance`), which the sim
  stores and rebroadcasts from our own `128` publish — so the bloat is
  self-perpetuating **per account**. Logging the account into a reference
  viewer (Firestorm) once overwrites the server appearance with the real
  worn-shape params and permanently corrects our render for that account; a
  never-corrected account stays bloated (confirmed: a second test avatar that
  never touched Firestorm stays bloated, a Firestorm-corrected one does not).
  The fix is the "matching the worn shape" work `bake_publish.rs` explicitly
  defers: resolve the own avatar's visual params from its worn **Shape**
  wearable (already fetched for baking) and use them for **both** rendering and
  the `AgentSetAppearance` publish, dropping the `128` placeholder — so the own
  avatar is correct on any account/grid regardless of server state and other
  viewers see the right shape. This is the *dominant* base-body appearance bug;
  the animation-time skin distortion (**R11**, whose skin-pivot premise turned
  out to be a proven sub-millimetre no-op) is a separate, smaller issue layered
  on top, to be tackled after this. Two viewer debug affordances were added to
  make this comparison possible: `--screenshot-dir` (an offline PNG capture
  harness that quits after N frames) and `--repeat-animation` (keep re-issuing
  `--play-animation` so a short motion still plays once the avatar has loaded).

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
