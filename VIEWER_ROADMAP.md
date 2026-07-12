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
  overwrote. **The limb distortion this originally noted (R11) is now fixed** —
  it was never the `LLSkinJoint` pivot scheme (a proven sub-millimetre no-op)
  but the R13 base-mesh render-list bug (extended-ancestor weight shift); with
  R13 in place the base body skins cleanly under animation (R11 verified).
- [x] **P18.4. Priority blending.** Resolve concurrently-playing animations
  per-joint by priority with ease-in/out transitions (higher priority wins a
  joint, blend on start/stop). Verify layered animations (e.g. an AO stand + a
  gesture) compose correctly. **Done.** Two new pure pieces in `sl-anim`
  (Bevy-free, unit-tested), mirroring the reference viewer: (1) the ease
  weighting — `Motion::pose_weight(elapsed, stopped_at)` reproduces
  `LLMotionController::updateMotionsByType`'s per-frame `setWeight` (cubic
  ease-in from activation, hold, cubic ease-out around the stop, the residual
  the ease-out scales captured at the stop so a stop mid-ease-in fades from the
  partial weight), a non-looping motion auto-easing-out to finish at its
  `duration` (the reference's `mSendStopTimestamp`), plus `is_finished` and a
  private `cubic_step`; (2) a `blend` module — `blend_joint(&mut
  [JointContribution]) -> BlendedJoint`, the pure counterpart of
  `LLJointStateBlender::blendJointStates`: order the per-joint contributions by
  priority (recency breaking ties), cap to the reference's four slots
  (`MAX_JOINT_CONTRIBUTIONS`), then fold each channel highest-priority-first
  (`new_sum = min(1, weight + sum)`, `nlerp`/`lerp` the accumulated toward the
  incoming by `sum / new_sum`) so a higher-priority motion dominates a joint
  while a lower-priority one shows through the unfilled weight, skipping
  zero-weight (fully-eased-out) contributions. `.anim` keyframe motions are
  always normal-blend, so the additive path is not modelled. The viewer's
  `animations.rs` driver was rewritten around this: a new `reconcile_playing`
  keeps each playing animation's start time and a per-avatar monotonic
  **activation-order** stamp, begins easing out (rather than dropping) an
  animation that leaves the authoritative set and retains it through its
  ease-out tail, and (re)activates a new or sequence-changed animation with a
  fresh stamp — assigned in **UUID order** within an update, which faithfully
  reproduces Second Life's equal-priority quirk (an observer present as each
  animation starts sees the last-*started* one win, because the reference pushes
  each new motion to the front of its active list; an observer arriving later
  starts them all at once from the sorted signalled set, so the highest-UUID one
  wins instead — the one stamping rule yields both). `drive_avatar_skeletons`
  then samples each playing motion, weights it by `pose_weight`, and blends the
  per-joint contributions via `blend_joint`; `PlayState` gained `stopped_at` /
  `order` and `AnimationPlayback` a `next_order` counter. To exercise it from
  one login, `--play-animation` is now **repeatable** (or comma-separated) so
  several animations layer at once. Verified live on OpenSim: the own avatar
  with `dance1` + `clap` layered blends cleanly — the clap's arm motion composes
  over the dance's full-body pose with no shearing, and the ease-in ramps the
  pose up smoothly on start.

## Phase 19 — Diagnostics HUD (FPS + pipeline status)

The rendering-fidelity phases below drive the fetch/decode pipeline much
harder, so the first new phase gives us the instruments to see it: an FPS /
frame-time readout and a live texture/mesh pipeline status panel. Reuses the
Phase 11 chat-overlay `bevy_ui` `Text` pattern (`chat.rs`).

- [x] **P19.1. FPS + frame-time overlay.** Add Bevy's
  `FrameTimeDiagnosticsPlugin`; render a `bevy_ui` text panel (the persistent
  absolute-positioned `Text` node pattern from `chat.rs`) showing FPS,
  frame-ms, and entity / draw counts. Reference: `LLViewerStats` /
  `LLFastTimerView` / `LLPerfStats`. **Done:** new viewer module
  `diagnostics.rs` — the viewer adds `FrameTimeDiagnosticsPlugin` +
  `EntityCountDiagnosticsPlugin`, a persistent top-left `Text` node (clear of
  the bottom-left chat overlay), rewritten each frame with the smoothed
  `FPS` / `FRAME_TIME` / `ENTITY_COUNT` diagnostics and a `draws` figure from
  the live `Mesh3d` instance count (a coarse per-frame draw-call gauge; Bevy
  has no draw-call diagnostic without the GPU-timing `RenderDiagnosticsPlugin`).
  Verified live on OpenSim: the overlay reads e.g. `FPS 60  (16.6 ms)` /
  `entities 1522  draws 1068`.
- [x] **P19.2. Pipeline status API (library).** The stores have no public
  introspection today (only per-request `TextureProgress` / `MeshProgress`).
  Add a public stats snapshot to `TextureStore` / `MeshStore` / `AssetStore`
  and `sl-asset-sched`'s `PriorityGate`: counts by state (queued /
  reading-disk / downloading / decoding / ready / failed), in-memory entries,
  cache hits, bytes, and GC'd entries — aggregated from the existing progress
  enums. Cross-cutting change across `sl-texture` / `sl-mesh` / `sl-asset` +
  `sl-asset-sched`; wire it through both runtime crates. Reference:
  `LLTextureFetch` / `LLMeshRepository` queue stats. **Done:** new
  `sl-asset-sched` `stats` module with a shared domain-free `StoreStats`
  (by-stage buckets + `in_memory` / `bytes` / `cache_hits` / `collected`) and a
  `GateStats` (capacity / in-flight / waiting) with `PriorityGate::stats()`.
  Each store gained a `stats()` (iterates its weak map, upgrades live entries,
  buckets them by their own progress enum, sums an approximate in-memory byte
  footprint) and a `gate_stats()`; new `cache_hits` / `collected` atomic
  counters are bumped on a disk hit and in `sweep`. `StoreStats` / `GateStats`
  re-exported once (via `sl_texture`) through both runtime crates. **Bug found
  & fixed while wiring stats through the progress enums:** the texture/mesh
  `get()` and `set_lod()` direct-fetch paths never published a terminal
  `Ready` / `Failed`, leaving an entry's observable progress stuck at the
  `Downloading` / `Decoding` it passed through (only the `request`/`drive`
  path published terminal progress). Extracted a shared `publish()` helper so
  every completion path leaves progress truthful. The `AssetStore` was
  unaffected — its single `get()` already published `Ready` / `Failed`.
- [x] **P19.3. Pipeline status overlay.** A key-toggled HUD panel rendering
  P19.2's texture + mesh pipeline counts (queued / decoding / ready / cached),
  so the LOD and priority work below can be watched live. **Done:** extended
  `diagnostics.rs` with a second `bevy_ui` `Text` node pinned top-left (clear of
  the top-right frame overlay and bottom-left chat), hidden by default and
  toggled with `F3` (new `PipelineOverlayVisible` resource +
  `PipelineStatusText` marker). While shown it is rewritten each frame from the
  P19.2 snapshots:
  `TextureManager` / `MeshManager` gained `stats()` / `gate_stats()` accessors
  delegating to their stores, and the panel prints two lines per pipeline —
  per-stage entry counts (queued / dl / dec / ready / fail) then the in-memory
  count + approximate byte footprint, cumulative `cached` (disk-cache hits) / GC
  counts, and the admission gate's in-flight/capacity/waiting. Byte footprint is
  rendered as MiB via integer math (the workspace denies `as` casts). An
  `SL_VIEWER_PIPELINE_OVERLAY` env var starts the panel visible so the offline
  screenshot harness (which cannot press `F3`) can capture it. Verified live on
  OpenSim: the panel reads e.g. `tex … cached 14 … gate 0/16 wait 0`. Reference:
  Firestorm `LLTextureFetch` / `LLMeshRepository` queue stats.

## Phase 20 — On-screen render priority

Everything is fetched at max fidelity in FIFO order today (textures at
`DiscardLevel::FULL`, meshes at `MeshLod::FINEST`), yet the schedulers already
support per-request priority (`sl-asset-sched` `Priority` +
`popularity_boost`, `TextureStore` / `MeshStore` `request(…, priority)` +
`.set_priority()`). This phase computes on-screen importance and feeds it, so
what the camera looks at loads first.

- [x] **P20.1. Screen-importance computation.** A Bevy-free helper computing
  an object / face's approximate screen pixel area from its world bounding
  radius, camera distance, viewport height, and vertical FOV. Port the
  reference viewer's `LLFace::getPixelArea` / `LLPipeline::calcPixelArea` /
  `LLVOVolume::getPixelArea`. **Done:** a new `screen` module in
  `sl-asset-sched` (the domain-free scheduling crate, so it sits next to the
  `Priority` P20.2 will map it onto) exposing `ScreenMetrics` — a per-frame
  `pixels_per_radian` factor (`window_height / vertical_fov`, the reference
  `LLDrawable::sCurPixelAngle`) built once and reused for every object, with
  `pixel_area(bounding_radius, camera_distance)` returning
  `(atan(radius/dist) * pixels_per_radian)² * π` (`LLPipeline::calcPixelArea`),
  including the near-object distance ramp (`dist < 16 m → (dist/16)²·16`).
  Guards a zero/degenerate FOV → 0 and a zero distance → the `pi/2` half-angle
  (matching `atan(+inf)`) instead of dividing by zero. Unit-tested; re-exported
  at the crate root.
- [x] **P20.2. Drive fetch priority.** Map pixel area (plus a boost for
  the own avatar / attachments / UI, mirroring `LLGLTexture::BOOST_*`) to a
  `sl_asset_sched::Priority`; feed it through `TextureStore::request` /
  `MeshStore::request` and re-prioritize each (throttled) frame via
  `.set_priority()` as the camera moves. The existing `popularity_boost`
  already lifts textures shared across many on-screen faces. Reference:
  `LLViewerTexture::addTextureStats`, the mesh `LODRequest` priority.
  **Done:** a new `Priority::from_pixel_area` in `sl-asset-sched` maps a P20.1
  pixel area to a scheduling priority exactly as the reference viewer's texture
  decode priority *is* its `mMaxVirtualSize` (clamped/rounded into the `u32`
  range, saturating at the full-resolution `2048 * 2048` area — the
  reference's `BOOST_HIGH` full-res force — exposed as
  `FULL_RESOLUTION_PIXEL_AREA`). The two viewer managers (`TextureManager` /
  `MeshManager`) now fetch through `store.request(…, priority).resolved()`
  instead of the ungated `store.get`, so every fetch is admitted through the
  store's 16-slot priority gate in on-screen order; each keeps its
  re-prioritizable request handle and gains a `set_priority`. A new
  `render_priority` module's `drive_render_priority` system recomputes, a few
  times a second (throttled 0.25 s), the pixel area every visible prim /
  sculpt / mesh face covers — keeping the *max* per texture (the reference's
  per-texture `mMaxVirtualSize`) — and the pixel area of each mesh object's
  still-fetching geometry, then feeds those back through `set_priority`, so
  what the camera looks at rises in the queue and what it turns away sinks
  (the driver's per-frame value is clamped — in *both* the texture and mesh
  managers — to never *demote* a request below its request-time base, so a boost
  is never undone by the face pass). Assets the face pass cannot rank from a
  scene object's pixel area are instead requested at a fixed boost: terrain
  detail textures (`BOOST_TERRAIN`), avatar textures / server bakes /
  client-bake layers, and — crucially — a **worn attachment's** face textures
  *and* mesh geometry (`BOOST_AVATAR`). An attachment is a skinned / joint-
  parented entity whose transform does not reflect its on-screen size, so the
  pixel-area pass ranks it too low; the boost (threaded through the geometry
  build from `worn_base_priority`, and unconditional for a rigged mesh) is what
  loads it with the avatar. Every boost sits in a band *strictly above* the
  pixel-area range (which saturates at the full-resolution `2048 * 2048`), so a
  boosted asset always outranks even the closest, largest prim rather than
  merely tying with it on a dense region. Verified live: OpenSim (terrain,
  prims, sculpt, textured avatar all load through the gated path) and aditi (a
  ~25k-entity region drove the gate genuinely under load — 270+ textures and
  440+ meshes queued, hundreds waiting — draining in on-screen order, with the
  center avatar's server bakes *and its worn mesh attachments* — jeans, top,
  hair — resolving ahead of the surrounding build).

## Phase 21 — Distance / pixel-area LOD

With per-object pixel area available (P20.1), fetch only the fidelity the view
warrants: coarser textures and meshes for small / distant objects, upgrading
as the camera approaches. The stores already expose `set_lod` for
upgrade/downgrade and the LOD newtypes have `finer()` / `coarser()`.

- [x] **P21.1. Texture discard-level selection.** From the P20.1 pixel area
  choose a `DiscardLevel` (fewer pixels → coarser); request at that level and
  upgrade / downgrade via `TextureStore::set_lod` as the camera approaches /
  recedes, respecting the read-lease. Reference:
  `LLViewerTexture::updateVirtualSize`. **Done:** a new
  `DiscardLevel::for_pixel_area` (`sl-proto`) ports the reference viewer's
  `discard = floor(log4(full_texels / on-screen area))`
  (`LLViewerLODTexture::processTextureStats`) — computed by repeated division by
  four rather than a float `log`, so a small / distant face selects a coarser
  level, using the texture's *native* (discard-0) dimensions so the same
  on-screen area maps to different levels for a 512² vs a 2048² texture. The
  `TextureManager` now splits its requests: an ordinary prim / mesh / sculpt
  diffuse face is **pixel-area LOD managed** (`request_face`) — first requested
  at a coarse placeholder level (`INITIAL_MANAGED_DISCARD`, ¼ linear) that loads
  fast, then upgraded / downgraded by the render-priority driver
  (`set_lod_for_area`, called alongside `set_priority` each throttled frame) via
  `TextureStore::set_lod` once the first decode reveals the native size. The
  store's `set_lod` fetches + decodes on an upgrade (growing the same codestream
  prefix — no re-fetch of bytes already in hand) and downsamples in place on a
  downgrade, waiting on the entry's GPU read-lease; the completed image is
  folded back in by `poll_textures` and re-uploaded *behind its existing Bevy
  image handle* (`build_prim_image` / `Assets::insert`), so every material
  sampling the texture shows the new resolution with no material re-patching.
  The initial
  request handle is retained for a managed texture (rather than dropped on
  resolve as in P20.2) so its store entry stays live for later `set_lod`.
  **Boosted textures stay full-resolution from the first fetch and are never LOD
  managed** — an avatar body part / bake, a worn attachment, a HUD attachment
  (all carry `AVATAR_BOOST` via `worn_base_priority`, which covers HUD
  attachment points), and terrain detail textures (`TERRAIN_BOOST`): a boosted
  request even *promotes* a texture a prim face had been managing
  (`upgrade_to_full`), so a shared id (e.g. a terrain texture reused on a prim)
  is never left coarse.
  Verified live: OpenSim (avatar + terrain render sharp, no regression) and a
  dense aditi region (441 LOD decisions — 280 downgrades, 161 upgrades — 0
  failures, 507 textures drained through the gate, the own avatar full-res and
  crisp at 60 FPS on a 35k-entity region).
- [x] **P21.2. Mesh LOD selection.** Port `LLVOVolume::calcLOD`: pick a
  `MeshLod` from pixel area / distance × `RenderVolumeLODFactor`, request that
  block, and swap on change via `MeshStore::set_lod`, rebuilding the Bevy
  mesh. Reference: `LLVolumeLODGroup`. **Done:** a new `MeshLod::for_distance`
  (`sl-proto`) ports `calcLOD` / `computeLODDetail` /
  `LLVolumeLODGroup::getDetailFromTan` — `tan_angle = lod_factor * radius /
  (distance * pi/3)` with the near-distance quadratic ramp, mapped through the
  `{1, 2, 8} * 0.03` thresholds; `radius` is the full scale-vector length
  (`getScale().length()`, **not** the half-diagonal used for pixel area — the
  reference thresholds are tuned against it), and a new `DEFAULT_LOD_FACTOR`
  (`RenderVolumeLODFactor`, `1.0`) is the quality knob. The `MeshManager` now
  splits its requests like the P21.1 texture manager: an ordinary scene mesh is
  fetched at a coarse `INITIAL_MANAGED_LOD` placeholder block and the
  render-priority driver upgrades / downgrades it (`set_lod_for_area`) toward
  the level its owning object's on-screen size warrants; a boosted worn
  attachment stays at `MeshLod::FINEST`, unmanaged. The driver aggregates the
  *finest* LOD any on-screen instance of a shared mesh needs (mirroring the
  per-texture max pixel area), so a mesh reused by many objects is not thrashed
  between levels by whichever instance is visited last. On a swap `set_lod`
  fetches + decodes the new block, `poll_meshes` re-announces the mesh, and
  `apply_object_meshes`
  despawns the object's old submesh entities and rebuilds them from the new
  geometry (fresh Bevy `Mesh` handles — so unlike the texture path there is no
  in-place-refresh problem). Verified live on aditi: a mesh drops to a coarser
  block as the camera recedes and rises again on approach. Verifying mesh LOD
  also surfaced and fixed **two latent P21.1 texture-LOD bugs**: (a) a
  full-resolution (discard 0) fetch used the `1/8`-rate byte *estimate*, which
  under-fetches a resolution-progressive codestream — the partial decode
  *succeeds*, so the decode-error fallback never fired and "full res" stuck at a
  reduced size; now a discard-0 fetch uses the guaranteed-complete
  `full_data_size_bound`, and the manager reads the true native size from the
  J2C header rather than back-calculating it; (b) a texture that changed LOD
  re-decoded but never *displayed* the new resolution, because `bevy_pbr` does
  not rebuild a material's bind group when an `Image` it samples is replaced —
  now the sampling materials are marked changed on re-upload. The crosshair pick
  tool (`P`) gained a live LOD readout (a face's texture discard level + true
  header-native size, and a mesh's decoded LOD) used to pin both bugs down; a
  512² texture was confirmed cycling `discard 0 → 3 → 0` (512² → 64² → 512²) and
  visibly re-sharpening on approach.
- [x] **P21.3. Prim LOD.** Replace the fixed `PrimLod::High` with
  a distance / area-selected `sl-prim` LOD tier (`LLVolumeLODGroup`);
  re-tessellate on change. **Done:** a new `PrimLod::for_distance`
  (`sl-prim`) selects the tessellation tier from radius / distance ×
  `RenderVolumeLODFactor`. The LOD-tier selection is the *same*
  `LLVolumeLODGroup` computation the reference viewer runs for a prim and a
  mesh (`LLVOVolume::calcLOD` picks a volume's detail before it matters
  whether the geometry is client-tessellated or asset-backed), so rather than
  duplicate the trig it delegates to the P21.2 `sl_proto::MeshLod::for_distance`
  and maps the resulting tier onto the matching `PrimLod` by index (both enums
  are coarsest-first with identical `0..=3` indices). A plain prim is now
  tessellated at a coarse `INITIAL_MANAGED_PRIM_LOD` placeholder and the
  render-priority driver refines it: `drive_render_priority` computes each
  prim's `PrimLod` from its full scale-vector length + camera distance (the same
  `getScale().length()` radius the mesh LOD pass uses, **not** the half-diagonal
  pixel-area radius) and records it in a new `PrimLodTargets` resource, which a
  new `apply_prim_lod` system drains to re-tessellate any prim whose desired
  level differs from its current one — the CPU-tessellation mirror of
  `apply_object_meshes`' fetch-driven mesh LOD swap, but with no async fetch
  (prim geometry is built on the spot). Each `TrackedObject` retains a
  `PendingPrim` (shape + texture entry + scale + priority) so a swap can rebuild
  without the live `Object`; only a plain prim carries it (a sculpt tessellates
  from its decoded map with no `PrimLod` input, a mesh from fetched blocks), so
  neither is prim-LOD managed. Since each prim tessellates its own shape there
  is no cross-instance aggregation (unlike a mesh asset shared by many objects).
  The crosshair pick tool (`P`) gained a prim-LOD readout alongside the P21.2
  mesh one. Verified live on OpenSim: the Default Region's prims each start at
  the `Low` placeholder and the driver upgrades them within a frame to
  `Medium` / `High` by on-screen size (a stack of tori / cylinders resolved to a
  mix of Medium and High, larger / nearer prims finer), no errors, avatar +
  terrain unaffected.

## Phase 22 — Sky & atmosphere (day cycle, EEP)

The scene has one static directional light today. This phase renders the SL
sky with its atmospheric model, driven by the region's Environment (EEP)
settings and animated through the day cycle. Its ingested settings also feed
Phase 23 (water) and Phase 24 (shadows).

- [x] **P22.1. Environment-settings ingest.** Parse region / parcel EEP
  settings (`LLSettingsSky` / `LLSettingsWater` / `LLSettingsDay`) with a
  legacy WindLight fallback, wired to the viewer through a new
  `EnvironmentUpdated` `SlEvent` (reuse the Phase 11 conformance environment
  work; keep the parse Bevy-free). Reference: `LLEnvironment`.

  **Done:** the parse + `Event::Environment` plumbing already existed from the
  Phase 11 conformance work (`environment_from_llsd` in `sl-proto`, surfaced to
  the viewer as `SlEvent(SessionEvent::Environment(..))` — no bespoke
  `EnvironmentUpdated` variant needed, the generic `SlEvent` wrapper already
  carries it). Net-new: a Bevy-free
  `EnvironmentSettings::legacy_windlight_default` (+
  `SkySettings::legacy_windlight_default` / `WaterSettings::legacy_default`)
  in `sl-proto`, transcribing Firestorm's `LLSettingsSky::defaults` /
  `LLSettingsWater::defaults` (incl. the legacy-haze `LLColor3`/`F32` fallbacks
  and the position-0 sun/moon `convert_azimuth_and_altitude_to_quat` tracks); a
  new viewer `EnvironmentState` resource (`environment.rs`) holding the current
  settings + provenance (`EnvironmentSource::{Default,Region,Parcel}`), starting
  at the legacy default; `request_environment` (asks for the whole-region
  environment on each `RegionHandshakeComplete`) and `ingest_environment` (folds
  the reply in, logs day length / offset / frame counts / cycle name). Also
  re-exported `SkySettings` / `WaterSettings` / `DayCycle` / `DayCycleFrame`
  from both runtime crates for parity (P22.2 needs them).

  **Model note (region = default, parcel = override, altitude = sky track):**
  the *region* environment is the baseline default; a *parcel* may override it
  where the region flags permit, and within either the day cycle carries up to
  four `sky_tracks` selected by camera altitude against `track_altitudes` (water
  is a single region-wide track). P22.1 ingests the region baseline; requesting
  the current parcel's override and picking the sky track by altitude are
  render-time concerns for P22.2/P22.3, which read the already-stored
  `EnvironmentSettings` (it carries `track_altitudes` + all `sky_tracks`).
- [x] **P22.2. Sky & atmosphere.** Render the atmospheric sky dome — port the
  Rayleigh / Mie scattering of `LLVOSky` / `LLVOWLSky` (+ the `skyV` / `skyF`
  deferred shaders) into a Bevy sky material; drive the sun / moon direction
  and colours, and set the scene directional light + ambient, from the sky.
  Select the active `sky_frames` entry by the camera's altitude against
  `EnvironmentSettings::track_altitudes` (region = default, parcel = override).
  Any sky / sun / moon / cloud / bloom / halo / rainbow texture the sky frame
  references must be fetched **boosted** through the texture manager
  (`request_boosted`, a new `SKY_BOOST_PRIORITY` mirroring `LLGLTexture::
  BOOST_HIGH`) so it resolves ahead of ordinary scene faces, exactly like the
  terrain / avatar textures.

  **Done (dome + lighting core; sun/moon disc, clouds and stars split out to
  P22.3–P22.5 below).** New `SkyMaterial` / `sky.wgsl` in `sl-client-bevy` (like
  `TerrainMaterial`, `bevy_pbr`-gated) transcribing the reference
  `class1/deferred/skyV.glsl` + `skyF.glsl` — the legacy two-colour exponential
  atmosphere (`blue_horizon` / `blue_density` / `haze_*` / `density_multiplier`
  / `max_y` / `glow` scattering with the anti-solar glow) plus the rainbow /
  halo overlays. The reference computes the haze colour *per vertex* on a
  tessellated dome; this evaluates the identical math *per fragment* on a
  camera-centred inward-facing sphere, so the sky is smooth without a dense
  mesh. New viewer `sky.rs`: `setup_sky` spawns the dome + the scene's sun/moon
  directional light; `center_sky_on_camera` keeps the dome on the camera;
  `drive_sky` selects the active `SkySettings` for the camera altitude (the
  reference `calculateSkyTrackForAltitude`, added Bevy-free as
  `EnvironmentSettings::sky_track_for_altitude` / `active_sky_settings`),
  computes the sun/moon direction + the scene light + ambient the way
  `LLSettingsSky::calculateLightSettings` does, and folds them into the
  material, the `DirectionalLight`, and the `GlobalAmbientLight`;
  `apply_sky_textures` swaps each decoded overlay in. `request_boosted` already
  existed from the P20 boost work, so the net-new was the `SKY_BOOST_PRIORITY`
  band (above the avatar boost) used for the rainbow / halo maps. Re-exported
  `Color` / `ColorAlpha` / `Glow` from both runtimes for parity. **Frame
  selection is time-*active*, not altitude-only:** the roadmap says altitude,
  but a single altitude track carries many day keyframes, so the active keyframe
  is picked at the current region day-position (`fmod(now + day_offset,
  day_length) / day_length`) *without* blending — the smooth keyframe
  interpolation is P22.6. Debug affordance `SL_VIEWER_SKY_DAY_POSITION` pins the
  day-position (0..1) so the offline screenshot harness can inspect any point in
  the day (verified midday on OpenSim: a blue dome, paler at the horizon from
  haze).
- [x] **P22.3. Sun & moon disc.** Render the sun and moon as textured billboards
  at their computed directions (the reference `sunDiscV/F.glsl` /
  `LLDrawPoolWLSky::renderHeavenlyBodies`), blended between the sky frame's two
  sun textures. Fetch the sky frame's `sun_texture` / `moon_texture` (or the
  reference defaults) **boosted** through the texture manager, as P22.2 already
  does for rainbow / halo.

  **Done.** New `SunDiscMaterial` / `sun_disc.wgsl` in `sl-client-bevy` (like
  `SkyMaterial`, `bevy_pbr`-gated) porting `sunDiscV/F.glsl` + `moonV/F.glsl`.
  It samples the disc texture (a `diffuse` / `alt_diffuse` pair blended by a
  `blend_factor` left at `0.0` until the day cycle drives it in P22.6), applies
  the moon's brightness, its transparent-pixel discard, and its near-horizon
  alpha fade, and is drawn `AlphaMode::Blend` over the (opaque) dome. **The
  reference does *not* tint the disc by its diffuse colour**: the CPU binds
  `DIFFUSE_COLOR` (sun) / `color` (moon) but `sunDiscF` never declares it and
  `moonF` declares yet never reads it, so both are dead uniforms; the disc
  shows its texture as-is (moon only scaled by `moon_brightness`). New viewer
  systems in `sky.rs`: `setup_sun_moon_discs` spawns two billboard quads (a
  shared unit `Rectangle` + a `SunDiscMaterial` each); `drive_sun_moon_discs`
  aims each disc at its Bevy-space direction (same `sky.{sun,moon}_rotation` as
  `drive_sky`) as a camera-facing billboard (the reference `hb_right` / `hb_up`
  basis + near-horizon enlargement, in `disc_transform`), sizes it by the
  reference `HEAVENLY_BODY_FACTOR` × disk radius × `{sun,moon}_scale` at a fixed
  `DISC_DISTANCE` (inside the dome so it depth-tests in front), shows only the
  bodies above the horizon (`getIsSunUp` / `getIsMoonUp`), and requests the
  `sun_texture` / `moon_texture` (or the built-in `DEFAULT_SUN_ID` /
  `DEFAULT_MOON_ID`) boosted at `SKY_BOOST_PRIORITY`; `apply_disc_textures`
  swaps each decoded disc in. Verified on OpenSim (pinned mid-day, camera aimed
  up: a bright glowing sun disc haloed into the atmosphere; the moon likewise).
  The A/B day-cycle texture blend is wired (`blend_factor`) but stays `0.0`
  until P22.6 supplies a next-frame texture.
- [x] **P22.4. Cloud layer.** Render the scrolling cloud layer — port
  `cloudsV/cloudsF.glsl` / `LLVOClouds` with the sky frame's
  `cloud_pos_density1/2`, `cloud_scale`, `cloud_scroll_rate`, `cloud_shadow`,
  `cloud_variance` and `cloud_color`, sampling the (boosted) `cloud_texture`.

  **Done.** New `CloudMaterial` / `clouds.wgsl` in `sl-client-bevy` (like
  `SkyMaterial`, `bevy_pbr`-gated) porting `cloudsV.glsl` + `cloudsF.glsl`. The
  reference computes the cloud lighting *per vertex* (`cloudsV`) and samples the
  multi-octave noise *per fragment* (`cloudsF`); this evaluates the whole thing
  *per fragment* on a camera-centred inward dome — the same approach `sky.wgsl`
  takes for the sky — so the clouds are smooth without a dense mesh. The cloud
  texcoords come from the reference dome's planar UV (`((-z + 1) / 2,
  (-x + 1) / 2)` of the view direction), here derived per fragment; the
  atmospheric inputs (`blue_horizon` / `blue_density` / `haze_*` /
  `density_multiplier` / `max_y` / `glow` / `sunlight_color` / `ambient_color` +
  `lightnorm`) are the sky frame's, so the cloud lighting matches the dome. New
  viewer systems in `sky.rs`: `setup_clouds` spawns the cloud dome (radius just
  inside `SKY_DOME_RADIUS` so the alpha-blended layer depth-tests in front of
  the opaque sky without z-fighting) + a `CloudMaterial`, `drive_clouds` folds
  the active sky frame into the material, accumulates the cloud scroll (the
  reference `LLEnvironment::updateCloudScroll`: `delta += dt *
  cloud_scroll_rate / 100`, folded into `cloud_pos_density1` with the x offset
  negated per `LLSettingsVOSky::applySpecial`), and requests the sky frame's
  `cloud_texture` (or the built-in `DEFAULT_CLOUD_ID`) boosted at
  `SKY_BOOST_PRIORITY`; `apply_cloud_textures` swaps the decoded noise in;
  `center_sky_on_camera` now follows both domes. **Key fix:** the cloud shader
  tiles the noise (`cloud_scale` magnifies the UVs and the `cloud_pos_density` /
  scroll offsets push them well outside `[0, 1]`), so the cloud image needs a
  **repeating** sampler — Bevy's default clamp-to-edge otherwise smears the
  black edge texel across the whole layer (noise sampled as `0` everywhere → no
  clouds, only a thin projection-stretch artifact); giving the cloud image
  `ImageAddressMode::Repeat` (as the prim/terrain textures already do, matching
  the reference `GL_REPEAT`) makes the noise tile. Verified on OpenSim (pinned
  mid-day): scattered white puffy clouds across the blue sky at the region's
  default coverage, denser as `cloud_shadow` rises. The A/B day-cycle noise
  blend is wired (`blend_factor`) but stays `0.0` until P22.6.
- [x] **P22.5. Stars.** Render the star field at night (the reference star
  pass / `star_brightness`), fading in as the sun sets.

  **Done.** New `StarMaterial` / `stars.wgsl` in `sl-client-bevy` (like
  `SunDiscMaterial`, `bevy_pbr`-gated) porting `class1/deferred/starsV.glsl` +
  `starsF.glsl` (`LLDrawPoolWLSky::renderStarsDeferred` /
  `LLVOWLSky::drawStars`). Unlike the sky / cloud domes (one inward sphere
  evaluated per fragment), the star field is **real quad geometry** — the
  viewer builds a mesh of 1000 star quads (the reference `getStarsNumVerts`),
  each a small camera-facing square with a per-star near-white colour, sampled
  from the sky's **bloom** texture (`IMG_BLOOM1`, the reference's star sprite —
  `getBloomTex`, boosted at `SKY_BOOST_PRIORITY`) and drawn **additively**
  (`AlphaMode::Add` = the reference `BT_ADD_WITH_ALPHA`) so the black bloom
  texels add nothing and only the bright star texels light the sky. The
  per-fragment `twinkle()` (a sawtooth of the model-space position scaled by
  `time`) and the `custom_alpha = star_brightness / 500` fade are the
  reference's; the field is hidden below the reference `0.001` threshold, so it
  fades in exactly as `star_brightness` rises through the day-cycle keyframes
  (smooth blend is P22.6). New viewer systems in `sky.rs`: `setup_stars` builds
  the deterministic (fixed-seed SplitMix64, standing in for `ll_frand`) star
  mesh; `drive_stars` centres the field on the camera, spins it very slowly
  about the up axis (the reference `rotatef(gFrameTimeSeconds * 0.01, …)` — in
  **degrees**, converted to radians), folds `star_brightness` / twinkle time
  into the material, and requests the bloom texture boosted;
  `apply_star_textures` swaps the decoded bloom in. **Star size:** the reference
  sizes its quads (`sc = 16 + frand * 20`) for its 15000 m `DOME_RADIUS`; ours
  sit at a nearer radius for screen projection, so the per-star size is scaled
  by `radius / 15000` to keep the same *angular* size (otherwise ~5× too big).

  **Far-plane skybox rework (cross-cutting, revisits P22.2–P22.4).** Stars
  exposed a latent depth limitation: the P22.2 sky dome was an **opaque
  world-space sphere at 3000 m that wrote depth**, so anything past ~3000 m from
  the camera (a 2000 m skybox, a tall build — content SL routinely has up to
  4096 m) was occluded by it, and stars had to sit inside it. Fixed by turning
  the sky, cloud, and star domes into a proper **skybox backdrop**: each vertex
  shader now forces its fragment to the reverse-Z far clip plane
  (`clip_position.z = 0`). Bevy's mesh pipeline uses a `GreaterEqual` depth
  test, so `0 >= 0` still draws the backdrop over the cleared (far) background
  while `0 >= any nearer geometry` fails — real scene geometry at **any**
  altitude now occludes the sky/clouds/stars, and the domes never hide objects
  beyond their own radius. The sun / moon discs deliberately keep their real
  2000 m world-space depth, so a disc still draws in front of the far-plane star
  field (occluding the stars behind it) — the reference's "moon writes depth to
  clip stars" intent. Verified on OpenSim (pinned night: pinpoint stars, moon,
  clouds, the own avatar correctly occluding the stars behind it; pinned midday:
  intact blue haze-graded sky, clouds, terrain, no stars).
- [x] **P22.6. Day cycle.** Interpolate the `LLSettingsDay` keyframes over
  region time (`getBlendedSettings`) to animate the sky and sun through the
  day, replacing P22.2's active-keyframe (unblended) selection with the smooth
  blend between the bounding keyframes.

  **Done.** Pure `sl-proto` addition, then a viewer swap. In
  `sl_proto::types::environment`: `SkySettings::blend(&self, other, factor)`
  interpolates one sky frame toward another the way the reference
  `LLSettingsBase::blend` does over the sky settings map — every numeric channel
  (haze scalars, colours, cloud/glow parameters, radii, star brightness, …) is
  linearly interpolated, the sun and moon rotations are **slerped** (the
  reference marks `sun_rotation` / `moon_rotation` as slerp keys — shortest-arc,
  with a normalised-lerp fallback for near-parallel inputs), and the discrete
  non-blendable settings (frame name + the six texture ids) snap to whichever
  frame is nearer (`factor > 0.5` picks `other`, matching the reference's
  `mix > 0.5 ? other : this`). A new private `bounding_keyframes(track,
  position)` finds the `(lower, upper)` day-cycle keyframes bracketing the
  current normalised time and the blend factor between their keyframe times,
  wrapping across the day boundary at both ends (upper wraps to the first frame
  after the last keyframe, lower to the last before the first) and
  special-casing a single-keyframe track to a factor-`0.0` self-blend; and
  `EnvironmentSettings::blended_sky_settings(altitude, position)` ties them
  together — selecting the altitude track (P22.2's
  `sky_track_for_altitude`), bracketing the position, and returning the blended
  (owned) `SkySettings`, falling back to any defined frame / holding the lower
  frame when the upper is missing. The unblended `active_sky_settings` is kept
  for the borrow-returning callers/tests. In the viewer, `sky.rs`'s five drivers
  (`setup_sky` / `drive_sky` / `drive_sun_moon_discs` / `drive_clouds` /
  `drive_stars`) now pull `blended_sky_settings` in place of
  `active_sky_settings` every frame, so the whole sky stack (dome atmosphere,
  scene sun/moon light + ambient, sun/moon discs, cloud layer, star field)
  animates continuously. 8 new `sl-proto` unit tests (bounding-keyframe
  bracketing + wrap + single-frame case; blend
  scalar/endpoint/slerp/texture-snap; `blended_sky_settings` interpolation +
  default-cycle no-op); `cargo test -p sl-proto` green (233).
  Verified live on OpenSim: the **Default region ships a real 8-sky-frame day
  cycle** (`day_length=14400s`, `day_offset=57600s`), so the blend is genuinely
  exercised — pinning `SL_VIEWER_SKY_DAY_POSITION` to `0.25` vs `0.75` renders
  two distinctly different skies (~7 % mean per-pixel difference; sky avg RGB
  `[211,235,255]` vs `[244,254,255]`) with the placeholder-sphere avatar visibly
  lit from a different sun direction (upper-left daylight vs shadowed), proving
  the interpolated sun rotation and sky settings drive the scene with no
  rendering regression.

## Phase 23 — Water surface

- [x] **P23.1. Water plane.** Render a water plane at the region water height
  with the EEP water settings (fresnel, reflection tint, scrolling wave
  normals) — `LLVOWater` / `LLSettingsWater` + the water shaders — as a custom
  Bevy material fed by P22.1's environment settings.

  **Done (surface + underwater fog), verified live on OpenSim.** Three layers,
  built to reproduce Firestorm as closely as the headless pipeline allows:

  *`sl-proto` (Bevy-free):* `WaterSettings::blend` (the day-cycle frame
  interpolation, the water counterpart of `SkySettings::blend` — lerps the
  fresnel / blur / fog / refraction scalars, the fog colour, the normal
  (wavelet) `Scale`, and the two wave directions; snaps name + normal /
  transparent textures at the half-way point), plus
  `EnvironmentSettings::active_water_settings(position)` /
  `blended_water_settings(position)` (water has **no** altitude tracks — one
  region-wide `water_track` — so unlike the sky they take only a day-cycle
  position). New `lerp_scale` helper; 5 new unit tests. `cargo test -p
  sl-proto` green.

  *`sl-client-bevy`:* new `WaterMaterial` / `water.wgsl` (`bevy_pbr`-gated, like
  the sky materials), a port of `class1/environment/waterV.glsl` +
  `class3/environment/waterF.glsl`. Per fragment it builds the three scrolling
  wave-normal texcoords (`waterV`'s sweeping displacement + `waveDir`/time
  scroll), samples the (blended `bumpMap`/`bumpMap2`) normal maps, and runs the
  reference `calculateFresnelFactors` (the `df3` three-term squared fresnel →
  reflection amount `df2.x`, plus `df2.y`) and `color = mix(fb, radiance,
  df2.x) + punctual`. The two G-buffer-dependent inputs the headless pipeline
  lacks are substituted by the reference's own fallbacks: `fb` (screen
  refraction) → the **water-fog colour** (exactly `applyWaterFogViewLinear` over
  white, the non-transparent-water path), and `radiance` (reflection probe) →
  a **sky reflection tint**; a Blinn-Phong sun glint stands in for the
  `pbrPunctual` specular.
  The per-wave fresnel dot is taken as `-abs(dot(view, wave))` so the surface
  shades as water from **both** faces (an underwater camera looking up at the
  underside reads as water, not a grazing sky reflection). Re-exported
  `WaterMaterial` / `WaterParams` / `WaterMaterialPlugin`.

  *Viewer `water.rs`:* per the reference `LLDrawPoolWater::render`, the water
  **colour / waves / fresnel are region-wide** (a single `getCurrentWater()` —
  the position-selected current EEP water — binds the whole water pass), so one
  **shared** material drives every plane; only the water **height** varies per
  region. `setup_water` spawns the **endless ocean** (a large camera-following
  plane at the agent region's water height — the reference hole/edge water that
  fills the sea wherever there is no loaded region); `drive_water` learns each
  region's water height from its `RegionInfoHandshake` and spawns a **per-region
  plane** for any neighbour whose height *differs* from the agent region's (a
  region with a different sea level renders at its own height; same-height
  regions are covered by the ocean, so the common case is one clean surface).
  Folds the blended EEP water settings + sun direction + a sky reflection tint +
  wave-scroll time into the shared material each frame and fetches the wave
  normal map (`DEFAULT_WATER_NORMAL` or the frame's own) boosted.

  *Viewer `underwater_fog.rs`:* a **fullscreen post-process** reproducing the
  reference water fog (`getWaterFogViewNoClip` / `applyWaterFogViewLinear`) over
  the *whole* scene — a per-material fog would miss objects / avatars, so this
  runs once on the composited image + the scene depth, fogging terrain, objects,
  avatars, and the water underside uniformly. It reconstructs each pixel's world
  position from depth and applies the reference's **per-fragment water-plane
  clip** (a fragment above the surface passes through untouched), so a camera
  straddling the surface splits cleanly along the waterline and underwater
  objects seen from above water still fog. `waterFogKS = 1 / max(lightDir.z,
  0.3)` and `getModifiedWaterFogDensity` (`pow(density, fogMod)` when the eye is
  submerged) are reproduced. The fog is applied after the main pass (display
  space, a pragmatic deviation from the reference's linear deferred stage; the
  distance falloff / clip are the reference's).

  **Bevy 0.19 render-API note (cross-cutting).** Bevy **0.19 replaced the render
  graph** with a system-based renderer (passes are systems in the `Core3d`
  schedule; `RenderContext` is a system param; pipelines specialize by the
  view's `target_format`; the `FullscreenMaterial` trait exists but its bind
  group is fixed to *(source, sampler, uniform)* with no depth binding). The fog
  is therefore a hand-written render system modelled on
  `bevy_post_process::effect_stack`. Depth comes from the **main-pass depth
  texture** made sampleable via `Camera3d::depth_texture_usages |=
  TEXTURE_BINDING` — **not** a `DepthPrepass`, because the prepass builds depth
  pipelines for the custom sky / terrain / water materials whose `specialize`
  pins bespoke vertex layouts that the prepass vertex shader rejects (a
  validation error); the main depth already carries every material's depth with
  no extra pipelines. The camera pins `Msaa::Sample4` so that depth texture is
  multisampled to match the fog's `texture_depth_2d_multisampled` binding. The
  three Bevy migration guides (0.16→0.17→0.18→0.19) are now referenced in the
  sl-client skill. **Deferred:** transparent-water refraction (seabed sharply
  through the surface) needs a screen-copy the headless viewer lacks; the
  clouds' vertical-orientation bug noticed here is tracked as **R18**.

## Phase 24 — Shadows

- [x] **P24.1. Sun / moon shadow maps.** Enable Bevy cascaded shadow maps on
  the directional light, driven by the P22.2 sky sun direction, with cascades
  tuned to region scale. Reference: `LLPipeline::renderShadow` /
  `RenderShadowDetail`. Done: `sky::setup_sky` enables `shadow_maps_enabled` on
  the `SceneSun` `DirectionalLight` and attaches a four-cascade
  `CascadeShadowConfig` reaching a region diagonal (~384 m) with a tight near
  cascade; `main` raises `DirectionalLightShadowMap` to 4096 for region-scale
  texel density. Prims and avatars (`StandardMaterial`) cast/receive out of the
  box, but the ground — the primary receiver — is the custom `TerrainMaterial`,
  so `terrain.wgsl` was reworked to read the shared view + light bind group:
  it now takes the sun/moon direction from the scene's first directional light
  (so the ground also tracks the day cycle, superseding its old hard-coded sun)
  and samples the cascaded shadow maps via `shadows::fetch_directional_shadow`,
  multiplying only the direct term by the shadow factor.

## Phase 25 — Local lights

- [x] **P25.1. Ingest light params.** Read a prim's light block (colour,
  radius, falloff, intensity, and spot cone params) from its light
  extra-params (`LLLightParams`). Done: a new viewer `lights` module decodes
  `object.extra.light` (+ the companion `light_image` when the prim is a
  spotlight/projector) into an `ObjectLight` component — linear RGB + intensity
  (the wire colour alpha, per `LLVOVolume::getLightIntensity`), radius, falloff,
  cutoff, and an optional `LightProjection` (texture + fov/focus/ambiance).
  `apply_object` inserts / refreshes / removes the component on every object
  update; the crosshair pick (`P`) reports the decoded light, and a `debug!`
  logs each ingest. Wire colour bytes are the **linear** colour (Firestorm's
  `LLLightParams::unpack` feeds them straight to `setLinearColor`), so no sRGB
  decode. Verified live on OpenSim against a provisioned orange point-light prim
  (`emitted=[0.8,0.398,0.0]`, i.e. linear `[1,0.5,0]` scaled by intensity `0.8`,
  radius 10 m, falloff 1). P25.2 will read `ObjectLight` to spawn Bevy lights.
- [x] **P25.2. Nearest-N selection + render.** Spawn Bevy `PointLight` /
  `SpotLight` for light-flagged prims, selecting the nearest / brightest N per
  frame within a budget (GPU / clustered-light limits). Reference:
  `LLPipeline::setupHWLights`, `LL_NUM_LIGHT_UNITS`. Done: `drive_local_lights`
  reads each frame's `ObjectLight` components (P25.1), ranks them by emitted
  luminance attenuated by camera distance (nearest / brightest first, mirroring
  `setupHWLights` keeping only the closest lights), and spawns the top
  `MAX_LOCAL_LIGHTS` (32) as Bevy lights — a `PointLight` for a plain light, a
  `SpotLight` (cone from the projector's half-FOV, inner cone from its focus)
  for a projector. Each Bevy light is a child of the light-flagged object
  entity with an identity local transform, so it rides the prim's transform
  and — for a
  spotlight — its forward (`-Z`) already equals Second Life's spot direction
  (`at_axis(0,0,-1) * render_rotation`) once the parent's coordinate conversion
  is applied, needing no extra rotation. The SL colour carries the light hue and
  the wire-alpha intensity rides Bevy's photometric lumens
  (`LOCAL_LIGHT_LUMENS = 1_000_000`, Bevy's `VERY_LARGE_CINEMA_LIGHT`), so
  radiance stays proportional to the emitted colour; the radius maps to the Bevy
  light `range`. Each Bevy light child is **kept alive and updated in place**
  across frames (tracked in a `LocalLights` object→child map, which also caches
  the last-applied light so an unchanged prim does no per-frame ECS mutation); a
  prim only gains a child on entering the budget and loses it on dropping out.
  Re-spawning (or even re-inserting the light component on) the child every
  frame churns the retained render world and makes the light *flicker* on lit
  surfaces — the reconcile-in-place-on-change path is what fixes that (verified
  live). A
  change in the rendered count logs once at `debug`. SL `falloff` has no Bevy
  analogue (Bevy's
  point light uses a fixed smooth range attenuation) and the projected light
  *texture* (`SpotLightTexture` / `PointLightTexture`) is not yet wired through
  the texture pipeline — both are follow-ups. Verified live on OpenSim: the
  provisioned orange point-light prim is selected (`rendering 1 of 1 candidate`)
  and rendered without regressing the scene.

## Phase 26 — Linden trees & grass

Trees and grass are classified `ObjectCategory::Other` and not rendered today.

- [x] **P26.1. Species table.** Port `app_settings/trees.xml` (the `LLVOTree`
  species table) as Bevy-free data. Done: a new **`sl-tree` crate** (the
  tree/grass counterpart of `sl-prim` / `sl-mesh` / `sl-sculpt`, Bevy-free and
  I/O-free) holds the 21-entry table in its `species` module — one
  `TreeSpecies` per species byte (diffuse `TextureKey` + every `LLVOTree`
  geometry parameter: droop / twist / branches / depth / scale_step /
  trunk_depth / branch/trunk length / leaf_scale / billboard scale+ratio /
  trunk+branch aspect / leaf_rotate / noise / taper / repeat_z), the
  `TREE_SPECIES` static, `MAX_TREE_SPECIES`, and a bounds-checked
  `tree_species(byte)` lookup. Values ported verbatim from `trees.xml`; as in
  Firestorm the `depth` / `trunk_depth` attributes parse as integers, so the
  fractional XML values (e.g. Fern's `trunk_depth="0.1"`) truncate toward zero.
  Unit-tested (index↔species_id, count, in/out-of-range lookup, texture ids,
  integer truncation). P26.2 will read this table to build the geometry.
- [x] **P26.2. Tree rendering.** Render pcode-tree objects as the reference
  branching geometry, falling back to a distance billboard imposter
  (`LLVOTree`), with the species diffuse texture through the texture pipeline.
  Done: `sl-tree` grew a Bevy-free `geometry` module porting
  `LLVOTree::updateGeometry` / `genBranchPipeline` — a recursive branch
  pipeline stamping transformed copies of a tapered trunk **cylinder** (4
  trunk LODs, the `sLODSlices` `{10,5,4,3}`) and a crossed-quad **leaf** card
  into one `TreeMesh`, in Second Life Z-up at unit outer scale, plus a
  `billboard_geometry` crossed-quad imposter. The trunk Perlin bark turbulence
  (`LLPerlinNoise::turbulence3`) is ported in an `sl-tree::noise` module that
  replicates glibc's TYPE_3 `random()` seeded with the C default `1` (what the
  reference's `init()` implicitly draws from, having no `srand()`) and consumes
  the stream in the same order (the `g1`/`g2`/`g3` draws then the permutation
  shuffle), unit-tested against the canonical glibc seed-1 sequence — so the
  bark matches a fresh-process reference. One faithful simplification remains:
  wind/trunk-bend is not simulated (so droop is the rest value
  `species.droop + 25°`). The winding, leaf-card layout, and the
  quaternion→matrix conventions are ported verbatim (unit-tested against the
  reference `LLQuaternion` vector-rotation formula). `sl-client-bevy` adds
  `to_bevy_tree_mesh` and re-exports the geometry API; the viewer gains an
  `ObjectCategory::Tree` (classified from `PCODE_TREE` / `PCODE_NEW_TREE`),
  builds one face entity textured with the species diffuse (a synthetic white
  `TextureFace` through the Phase-6 pipeline, `AlphaMode::Mask` so the
  leaf-card / trunk alpha clips cutout foliage), and applies the reference
  tree placement in a tree-specific geometry-holder transform (uniform
  `scale.length() * 0.05` scale, 90° Z yaw, `-0.1 m` plant nudge). The
  render-priority driver picks each tree's `TreeTier` from its on-screen
  size — the branching LOD by distance, or the billboard imposter once tiny —
  and `apply_tree_lod` regenerates on a change, the tree counterpart of the
  prim LOD path. Verified live on OpenSim (a `rez_sample_trees` example rezzes
  a stand of species): trunk bark + cutout leaf cards render correctly. Two
  live findings baked in: OpenSim's vegetation module multiplies a rezzed
  tree's scale by ~8 (`AdaptTree`), and the species texture is an atlas whose
  transparent edges made a repeat-wrapped bilinear sample bleed through the
  alpha mask at the trunk seam — fixed by a small `TRUNK_U_MARGIN` inset on
  the seam column.
- [x] **P26.3. Grass.** Render pcode-grass as the reference
  crossed-quad patches (`LLVOGrass`) with the species texture. Done: `sl-tree`
  grew a Bevy-free `grass` module porting `LLVOGrass::getGeometry` /
  `LLVOGrass::initClass` — a fan of up to `GRASS_MAX_BLADES` (32) leaning
  two-sided blade *cards* (8 vertices / 12 indices each, front and back copies
  with opposite normals) scattered around the object centre with a Gaussian
  spread, into one `GrassMesh` — plus a `grass` species table (`GrassSpecies` /
  `GRASS_SPECIES`, 6 entries) ported from `app_settings/grass.xml` (diffuse
  texture + `blade_size_x` / `blade_size_y`), with a `grass_species` lookup. The
  reference multiplies the blade-centre scatter by the object scale (`x =
  exp_x * mScale`) but sizes each card from the species table, so the object
  scale is folded into the *spread* inside the generated geometry (absolute
  metres), **not** applied as a mesh scale — the winding, the leaning `- xf`
  base-2 quirk, the forced `+0.75` blade-normal Z, and the `u`/`v` card UVs are
  ported verbatim, unit-tested for counts / clamping / scale-spread.
  `sl-client-bevy` adds `to_bevy_grass_mesh` and re-exports the grass API; the
  viewer gains an `ObjectCategory::Grass` (classified from `PCODE_GRASS`) and
  builds one face entity textured with the species diffuse (a synthetic white
  `TextureFace` through the Phase-6 pipeline, `AlphaMode::Blend` to match the
  reference's `PASS_GRASS` / `POOL_ALPHA` soft-edged blades), placed by an
  **identity** geometry-holder transform (the object scale already lives in the
  mesh spread). Since a grass clump's geometry depends on the object scale —
  where a prim's / tree's does not — the object's X/Y scale is folded into a
  grass-only field of the geometry-rebuild `ShapeFingerprint` (quantised to
  mm), so a live resize rebuilds the clump while never re-tessellating any
  other category. Verified live on OpenSim (a new `rez_sample_grass` example
  rezzes a row of all six species): the blade fans render as upright wispy
  grass with the species texture. Three faithful simplifications, documented in
  the module: blade bases sit on the object's local `z = 0` plane rather than
  each sampling the terrain height (`resolveHeightRegion`, needs a heightfield
  this I/O-free crate lacks); the per-blade scatter comes from a fixed-seed PRNG
  reproducing the reference's Box–Muller distribution (the reference seeds
  `ll_frand` from a *random* UUID, so its exact layout differs every run and is
  shared by all grass — we likewise share one stable layout); and wind sway is
  not simulated. No blade-count distance LOD (the reference sheds blades for
  performance; not required here).

## Phase 27 — PBR & legacy materials

Faces use a diffuse-only `StandardMaterial` today. This phase adds the modern
GLTF PBR materials and the pre-PBR legacy material stack, both of which Bevy's
`StandardMaterial` can largely express.

- [x] **P27.1. GLTF PBR materials.** Fetch `LLFetchedGLTFMaterial` assets and
  map to Bevy `StandardMaterial` (base colour, metallic-roughness, normal,
  emissive, occlusion, alpha mode / cutoff, double-sided), with each map
  supplied by the texture pipeline. Reference: `LLGLTFMaterial`. **Done:** a new
  pure crate **`sl-material`** decodes the `AT_MATERIAL` asset (an LLSD envelope
  `{ version, type, data }` wrapping a glTF 2.0 document) into a
  renderer-agnostic `GltfMaterial` — base-colour / metallic / roughness /
  emissive factors, four texture slots with `KHR_texture_transform`, alpha
  mode + cutoff, double-sided — re-exported from both runtimes. A new viewer
  module `materials.rs` owns a `MaterialManager` (its own `AssetStore` over the
  `ViewerAsset` cap, mirroring the animation / wearable pipelines): a face's
  base PBR material id comes from the object's `render_material` extra-params
  (`LLRenderMaterialParams`), attached to the geometry-holder entity as
  `ObjectRenderMaterials` so `register_pbr_materials` joins each spawned face to
  it; the manager fetches + decodes the asset, patches the face
  `StandardMaterial`'s scalar fields, and requests each map through the shared
  `TextureManager` (base colour / emissive uploaded sRGB, normal /
  metallic-roughness linear; the ORM map drives both the metallic-roughness and
  occlusion slots). Bevy carries a single `uv_transform`, so the base-colour
  `KHR_texture_transform` composes onto the face's texture-entry placement and
  stands in for every slot (an approximation of the reference's per-slot
  transforms). Decoder unit-tested (`cargo test -p sl-material`). Live check:
  the pipeline runs clean on both OpenSim and aditi with no regression, but
  neither reachable login point had a GLTF-PBR-material prim in view, so an
  on-screen PBR render is not yet confirmed against real content (OpenSim's
  Default Region carries none; the aditi landing region showed none). Per-face
  **overrides** are P27.2; **terrain** PBR (the R15 single-colour-terrain
  suspect) is a separate path, not this prim/mesh-face material.
- [x] **P27.2. GLTF material overrides.** Apply per-face
  `GltfMaterialOverride` deltas delivered via the override cap / ObjectUpdate
  extended data, layered on the base material. Reference:
  `LLGLTFMaterialList::applyOverride`. **Done:** the simulator pushes per-face
  overrides in a GLTF material-override `GenericStreamingMessage` (method
  `0x4175`), already surfaced by `sl-proto` as
  `Event::GltfMaterialOverride { local_id, faces, overrides }` with each face's
  override document left as raw notation-LLSD bytes. Net-new decoding: a new
  **`parse_llsd_notation`** in `sl-llsd` (the textual counterpart of the binary
  parser — every LLSD kind, mirroring Firestorm's `LLSDNotationParser`), and in
  `sl-material` a **`MaterialOverride`** sparse-delta type with
  `parse_material_override` (decodes one `od[i]` notation map — the shaved
  `tex`/`bc`/`ec`/`mf`/`rf`/`am`/`ac`/`ds`/`ti` keys) and `apply_to` (folds the
  delta onto a base `GltfMaterial`, mirroring `applyOverrideLLSD` +
  `applyOverride`: the `GLTF_OVERRIDE_NULL_UUID` sentinel clears a texture slot,
  a present factor replaces the base's, per-slot transforms fold on). Both
  re-exported from the two runtimes. In the viewer, `materials.rs` gained a
  scoped-object + face-index key on each registered PBR face
  (`ObjectRenderMaterials` now carries the object's `scoped_id`) and a
  **recompose** model: the base material and any stored override are re-applied
  together whenever either changes (base decode, or a new
  `apply_material_overrides` system that decodes + stores/clears the per-face
  overrides and reverts faces the message omits). The face's diffuse
  `uv_transform` is captured at registration so recomposition never
  double-composes the base-colour `KHR_texture_transform`. Decoders unit-tested
  (`sl-llsd`, `sl-material`). **Live-confirmed on aditi** (unlike P27.1): the
  landing region pushed real overrides (two objects, 4 + 1 faces) that flowed
  through the pipeline cleanly — though the base maps could not be shown because
  aditi's `ViewerAsset` service 503s (the same flakiness that left the asset /
  bake cases aditi-partial). OpenSim's Default Region carries no PBR/override
  content, so no on-screen confirmation there.
- [x] **P27.3. Legacy materials (normal / specular).** Support the pre-PBR
  `LLMaterial` (RenderMaterials): normal map + specular map +
  environment / glossiness + alpha mode, mapped onto `StandardMaterial` normal
  / metallic where possible. Reference: `LLMaterialMgr` / `lldrawpoolmaterials`.
  **Done:** the whole wire/proto/runtime half already existed (the `sl-wire`
  `LegacyMaterial` / `RenderMaterialEntry` codec over the zipped binary-LLSD
  `RenderMaterials` capability, `sl-proto`'s `Event::RenderMaterials`, the
  `RequestRenderMaterials` command in both runtimes) — net-new was purely the
  viewer application layer, a new `legacy_materials.rs` module mirroring the
  P27.1 PBR pipeline but driven by the capability's **batch** fetch rather than
  a per-asset `ViewerAsset` fetch. A face references a legacy material by the
  16-byte `material_id` in its `TextureEntry` face (already carried on each face
  entity as `FaceTextureDebug`); `register_legacy_materials` picks up each
  newly-spawned face carrying one — skipping any face that also has a PBR GLTF
  material, which supersedes it as in the reference — and queues the id.
  `drive_legacy_material_requests` batches the outstanding ids into
  `RequestRenderMaterials` commands (chunked to the reference's
  50-per-transaction limit), `receive_legacy_materials` folds the decoded reply
  into a cache, and `apply_legacy_materials` writes each material onto the
  waiting faces + requests its normal map through the shared `TextureManager`
  (`apply_legacy_normal_maps` uploads the map linear into the normal slot). The
  `StandardMaterial` mapping is faithful for the **normal map**; the specular /
  environment / glossiness stack is folded into the scalar `reflectance` (from
  environment intensity) and `perceptual_roughness` (from the specular
  exponent / glossiness), and the diffuse alpha mode maps `NONE`→opaque and
  `MASK`→alpha-test (leaving `BLEND` / `EMISSIVE` to the diffuse-derived mode
  so a legacy material never forces an opaque face into the z-sorted transparent
  path). Documented approximations (Bevy's `StandardMaterial` cannot express
  them): the specular **map texture** and the per-map (normal / specular) UV
  transforms are dropped, and the specular colour tint is not applied. Scalar
  conversions unit-tested.
  **Live-confirmed on aditi** (like P27.2): the landing region drove a clean
  round-trip of **63 legacy materials requested = 63 received** over the
  `RenderMaterials` cap (which — unlike the `ViewerAsset` cap that left the
  asset / bake cases aditi-partial — works on aditi) with the scene rendering
  intact. OpenSim's Default Region carries no legacy-material faces, so no
  on-screen confirmation there (the pipeline runs clean).
- [x] **P27.4. Bump / shiny / glow / fullbright.** The legacy per-face
  bump / shiny / fullbright / glow flags → Bevy emissive / normal / metallic
  approximations. Reference: `lldrawpoolbump` / `LLFace::getGeometryVolume` /
  the `SHININESS_TO_ALPHA` shiny packing. **Done:** a new `bump.rs` module maps
  the four legacy surface effects a `TextureEntry` face carries (in its
  `bump_shiny_fullbright` byte plus the separate `glow` scalar — the pre-PBR
  per-face controls, distinct from the P27.1 GLTF and P27.3 `LLMaterial`
  materials). The scalar three fold onto each face's `StandardMaterial` as it is
  built, by `apply_surface_flags` called from `face_material`, so they cover
  prims, sculpts, meshes, and rigged attachments uniformly: **fullbright** →
  `unlit` (exact); **glow** (0..1) → an additive `emissive` tinted by the face
  colour (the viewer has no bloom pass, so a glowing face simply reads brighter,
  and the glow is uniform rather than texture-following — a documented
  approximation); **shiny** (none / low / medium / high) → an *analytic-light*
  highlight, not a cube-map reflection, since the viewer has no reflection
  probe (a metallic surface would read black) — `reflectance` is raised and
  `perceptual_roughness` lowered with the level (driven by the reference's
  `SHININESS_TO_ALPHA = [0, .25, .5, .75]` environment-intensity table), leaving
  metallic at zero so the sun/moon directional light throws a progressively
  sharper, brighter specular. **Bump** needs the decoded diffuse, so it runs as
  a small fetch/generate pipeline like the P27.3 normal path: a `BumpManager`
  resource, `register_bump_faces` (parks each newly-spawned bumped face on its
  diffuse texture id, skipping a face with no diffuse, a legacy `LLMaterial` id
  — P27.3 supplies its normal — or a PBR GLTF material, which supersedes the
  legacy flags as in the reference), and `apply_bump_normals` (once the diffuse
  decodes, generates a tangent-space **normal map** from its luminance as a
  height field — Sobel central differences, wrapping to match the repeating face
  sampler — and drops it into `normal_map_texture`). The normal's **source**
  matches the reference: the brightness / darkness codes derive it from the
  face's own diffuse (darkness inverts the height field), while the 15 standard
  emboss codes (≥ 3 — woodgrain, bark, bricks, …) fetch their fixed Linden bump
  texture (the reference viewer's `std_bump.ini` UUID table) through the shared
  texture manager and derive the normal from that. Runs after the legacy
  material path so a real `LLMaterial` normal wins. Scalar mappings + normal
  encoding + the standard-code lookup unit-tested. **Live-confirmed on aditi**
  (like P27.2 / P27.3): the landing region drove real bump content — dozens of
  faces across many textures generated normal maps cleanly (6 / 8 / 16 / 116 …
  faces per texture), including the real standard emboss textures (woodgrain,
  gravel, siding fetched by UUID), with the scene rendering intact.
  OpenSim's Default Region carries no bump/shiny faces, so no on-screen
  confirmation there (the pipeline runs clean).

## Phase 28 — Animated textures

Prims animate their textures (`llSetTextureAnim`): UV scroll / rotate / scale,
or a sprite-sheet flipbook stepping through a grid of frames. The wire block is
already decoded — `sl-proto`'s `decode_texture_anim` → `TextureAnimation` (mode
flags, `face`, the `size_x` × `size_y` frame grid, `start`, `length`, `rate`) —
but nothing in the viewer consumes it, so every animated texture currently sits
static. This phase is the viewer-side driver. Reference: `LLViewerTextureAnim` /
`LLVOVolume::animateTextures`.

- [x] **P28.1. Ingest per-object texture animation.** Carry the decoded
  `TextureAnimation` from each object's `texture_anim` update onto the object
  (a component beside the geometry holder, like the P27 material holders),
  resolving the target-face bitmask (`face == -1` = all faces). The decode
  itself already lives in `sl-proto`; net-new is holding the state on the object
  and clearing it when the animation stops (`ON` bit clear). **Done:** a new
  `texture_anim.rs` module holds an `ObjectTextureAnimation` component — the
  decoded `TextureAnimation` — on the object's **geometry holder** entity (the
  parent of its face entities), exactly mirroring the P27.1
  `ObjectRenderMaterials` holder. `apply_texture_animation` (in `objects.rs`,
  beside `apply_render_materials`) refreshes it on every object update, gated by
  `running_texture_animation` so the holder is present only while the `ON` bit
  is set and removed otherwise — a prim whose animation is stopped in-world
  reverts to static. The `-1` = all-faces resolution lives in
  `ObjectTextureAnimation::applies_to_face` (taking a `u16` face index so it
  also covers mesh faces past the prim range), which the P28.2 driver will use
  to pick affected faces; unit-tested along with the `ON`-gate. Since a terse
  update clones the session's cached full `Object`, the decoded animation
  survives motion-only updates (no risk of a terse update wrongly clearing it,
  which would flip the animation static every frame). No visual
  change yet (that is P28.2) — the ingest is surfaced by a `debug!` on each
  ingest and by the `P` pick tool, which reports the picked object's animation
  params and whether it targets the face under the crosshair. **Live-confirmed
  on both grids:** OpenSim drove the ingest `debug!` from a provisioned
  `slclient-texanim.oar` prim (`mode=0x03` `ON|LOOP`, `2x2` flipbook grid), and
  aditi's pick tool read a real scrolling prim (`mode=0x13` `ON|LOOP|SMOOTH`,
  `1x1` grid, `rate=0.300`, `targets_picked_face=Some(true)`).
- [x] **P28.2. Drive the animation.** Each frame advance every animated
  object and update its affected faces: the `ROTATE` / `SCALE` / scroll modes
  compose an extra UV transform onto the face's texture-entry placement
  (`StandardMaterial::uv_transform`), while the flipbook mode selects the
  current cell of the `size_x` × `size_y` sprite grid (a per-cell offset +
  scale),
  honouring the `LOOP` / `REVERSE` / `PING_PONG` / `SMOOTH` mode flags and the
  `start` / `length` / `rate` timing. Mirrors the reference viewer's
  `LLVOVolume::animateTextures` folding a per-face texture matrix each frame.
  **Done:** `drive_texture_animations` (in `texture_anim.rs`) advances every
  `ObjectTextureAnimation` holder each frame — an accumulated-elapsed
  `TextureAnimationClock` beside the holder (restarted on a re-parameterised
  `llSetTextureAnim`) feeds a faithful port of
  `LLViewerTextureAnim::animateTextures` (`animate` → an `AnimatedPlacement` of
  the driven offset / scale / rotation, the un-driven components falling back to
  the face's static `TextureFace`), which is folded into each affected face's
  `StandardMaterial::uv_transform` via the new param-based
  `texture_uv_transform` (the factored-out core of `texture_face_uv_transform`,
  now shared). The animation *replaces* the face's UV transform exactly as the
  reference viewer uses `mTextureMatrix` instead of the static xform while
  running (confirmed against `LLFace::getGeometryVolume`'s `do_tex_mat` path);
  `restore_stopped_animations` resets each face back to its static placement
  (and drops the clock) when the `ObjectTextureAnimation` holder is removed (the
  `ON` bit cleared in-world, or the prim gone), via `RemovedComponents`. The
  port's flipbook cell-selection / non-loop clamp / scroll / rotate paths are
  unit-tested. **Live-confirmed on aditi:** the real scrolling /
  animated-texture prims are visibly animated. On the local OpenSim the
  provisioned
  `slclient-texanim.oar` prim ingests and drives correctly (mode=0x03 ON|LOOP,
  2×2, rate 1, length 4) but its default texture is the synthetic placeholder
  `00000000-0000-1111-9999-000000000005` (no real asset), so the flipbook
  cell-stepping has no image content to reveal and looks static — an
  untextured-prim artifact of that fixture, not the driver.

## Phase 29 — Animesh

Animated-object linksets are detected (`is_animated_object`) but rendered as
plain prims. This phase gives them their own animation-driven skeleton.

- [x] **P29.1. Control-avatar skeleton.** Give an animated-object linkset its
  own `LLControlAvatar` skeleton, built from the linkset's rigged-mesh skin
  joints and independent of any wearer. **Done.** A new viewer module
  `animesh.rs` owns a `ControlAvatarState` resource: one *control avatar* per
  animated-object root, keyed by the root's full `ObjectKey` (the id
  `ObjectAnimation` names). Rather than re-deriving a skeleton from the
  linkset's skin, the control avatar reuses the **standard** avatar skeleton
  (the reference `LLControlAvatar` inherits the full `LLVOAvatar` skeleton, and
  a rigged mesh binds to it by joint name exactly as a worn one does) via a new
  `AvatarBody::spawn_bare_skeleton` — the joint-spawning half of
  `AvatarState::spawn_body` with no base-body parts, attachment nodes, or name
  tag. The skeleton root is an **identity child of the animesh root object
  entity**, so the whole skeleton follows the object's Bevy world transform
  (which already carries the Second Life → Bevy basis change + world
  placement/rotation) and despawns with it — the reference viewer's
  `matchVolumeTransform` pins the control avatar to the root prim's render pose
  (the bind-shape rotation it also folds in is already carried by our rigged
  skinning's inverse bindposes, so it is not re-applied).
  `apply_rigged_attachments` now branches: an animesh linkset's rigged meshes
  (detected by walking the parent chain to the animated-object root via the new
  `animesh_root`, replacing the old `belongs_to_animesh` predicate) bind to the
  control avatar's joints — spawned on demand at first bind via
  `ControlAvatarState::ensure_spawned` — instead of a wearer's, with the wearer
  agent passed as `None` (an animesh has no wearer bake, so its faces texture
  from ordinary fetches, never bake-on-mesh). The rig's joint position overrides
  (R1) are recorded on the control avatar rather than any wearer.
  `prune_control_avatars` drops a control avatar whose root object is gone (its
  entities already despawned with the object). Net-new library change was only
  re-exporting `ObjectKey` / `ObjectPlayingAnimation` from `sl-client-bevy` and
  adding `full_key: ObjectKey` to the viewer's `TrackedObject`. **A rigged-mesh
  LOD-race fix fell out of this and is load-bearing:** an animesh is not an
  attachment, so its mesh starts on the managed coarse-LOD path; the finest-LOD
  upgrade (`upgrade_to_finest`) is async, but `apply_rigged_attachments` was
  binding whatever `decoded()` returned *now* (the coarse 4-vertex block), and
  rigged meshes are excluded from the LOD-swap rebuild — so the animesh rendered
  as a collapsed few-vertex husk. `apply_rigged_attachments` now waits on
  `MeshManager::lod_change_inflight(key)` before binding, so it always builds
  the finest geometry. **Verified live on aditi:** the two "King Kong"
  Super-Mario animesh render as correct, full-resolution rigged meshes
  (previously a transparent-outline husk / single triangle).
- [ ] **P29.2. Drive its animations.** Route the object's animation state
  (`ObjectAnimation`) through the Phase 18 blend driver against that skeleton so
  the rigged mesh deforms. Reuses the Phase 12 skeleton and Phase 18 blend.
  Reference: `LLControlAvatar` / `LLDrawPoolAvatar`. **Implemented but NOT yet
  observed animating live — blocked on `ObjectAnimation` delivery / object
  tracking, needs a wire-capture investigation.** The driving pipeline is in
  place and correct: the three per-avatar animation helpers were extracted from
  `animations.rs` as shared `pub(crate)` functions — `reconcile_playing` (now
  taking `(anim_id, sequence_id)` pairs so both `PlayingAnimation` and
  `ObjectPlayingAnimation` drive it), `retain_active`, and `resolve_pose`
  (sample + priority-blend a playing set into an `AnimationPose` with a
  joint-name→index resolver) — and the avatar driver now calls them too, so the
  animesh path shares the exact ease-in/out + priority-blend logic.
  `ingest_object_animations` fetches each signalled motion through the **same**
  `AnimationManager`; `drive_control_avatars` folds each object's
  `ObjectAnimation` into a per-object playback clock and blends a pose (names
  via the shared `AvatarBody::joint_index`); `pose_control_avatars` (in
  `PostUpdate`, after propagation, beside `pose_avatar_skeletons`) re-runs the
  SL skeletal recurrence with a **rest** `SkeletalDeformations` + the linkset's
  joint overrides and writes each joint's world matrix.
  `spawn_animesh_control_avatars` spawns a control avatar as soon as an object
  has an animation playing (not only when its mesh binds), so an animation
  arriving before the mesh decode is not lost. **Live-verified on
  fetch/decode:** the signalled custom `.anim` motions fetch and decode fine (no
  errors). **But no animesh actually animates**, because the `ObjectAnimation`s
  the sim sends do not correspond to the animesh we track and render:
  - of the animated objects an aditi region signalled, **~15 of 17 were never
    tracked** by us at all (an `ObjectAnimation` arrives but no `ObjectUpdate`
    ever does) — most likely animesh **attachments on the coarse / distant
    avatars** (whose wearer is not streamed as a full object, so neither are its
    attachments), since the region had no fully-rendered neighbour avatars;
  - the few we *do* track are **linkset children with no animated flag**
    (`is_root=false, animated=false`), so `animesh_root` / the early-spawn never
    key a control avatar to them; and
  - the in-world Mario animesh we *do* track as animated roots (and spawn
    control avatars for) receive **zero** `ObjectAnimation`, even after the
    capability fix below — so the sim is not streaming their (looping, set-once)
    animation to us.

  Fixes made along the way that **did** land (all build/clippy/test clean, no
  OpenSim login regression): (1) the viewer now requests the **`ObjectAnimation`
  capability** in its seed-caps list (`CAP_OBJECT_ANIMATION`) — the sim
  withholds the `ObjectAnimation` UDP stream from a viewer that did not
  advertise animesh support, which is why we saw *zero* animation events before;
  this made many more arrive. (2) `Session::dispatch_child` now handles
  **`AvatarAnimation` / `ObjectAnimation` on child (neighbour-region) circuits**
  — they were falling through to the unhandled-message diagnostic, so
  neighbour-region avatars and
  animesh could never animate. (3) `CompleteAgentMovement` is now **deferred
  until the region's capabilities are fetched** (both runtimes) so the sim knows
  we render animesh before it streams the scene — did not by itself unblock the
  Mario, but is correct in general and fails login cleanly if caps never arrive.
  **Next step:** a `tcpdump` of an aditi session run through
  `sl-conformance-trace` to correlate the `ObjectAnimation.object_id`s against
  `ObjectUpdate` ids — to settle "the sim never streams these objects to us" vs.
  "we track them but key them wrong", and to see why the tracked Mario roots get
  no `ObjectAnimation`.

## Phase 30 — Particles

- [x] **P30.1. Ingest particle systems.** Parse a prim's `LLPartSysData` (the
  particle-system block on ObjectUpdate / generic data): flags, pattern,
  burst / age params, per-particle colour / scale / velocity ranges, target.
  Keep it Bevy-free where practical. Reference: `LLPartSysData` / `LLPartData`.
  The Bevy-free wire decode already existed in `sl-proto`
  (`decode_particle_system` → `ParticleSystem`, both the legacy 86-byte and the
  modern size-prefixed glow/blend-extended forms, on `Object::particles`), so
  the net-new work was the **viewer-side ingest**, mirroring the P25.1 light
  ingest exactly: a new `sl-client-bevy-viewer::particles` module with an
  `ObjectParticleSystem` component carrying the decoded system, a
  `particles_from_object` lift, and an `apply_particles` reconcile that
  `apply_object` calls on both the spawn and update paths (beside `apply_light`)
  so a source toggled on/off/retuned between updates is tracked. The lift
  honours the reference viewer's `LLPartSysData::isNullPS` semantics — an empty
  `PSBlock` (sl-proto already yields `None`) **and** a zero-CRC "null" system
  (the `llParticleSystem([])` stop sentinel) both clear the component rather
  than attach a dead emitter, matching `LLViewerPartSourceScript::unpackPSS`
  returning `NULL`. The component rides the **object entity** (its world
  transform), the way `LLViewerPartSourceScript` tracks its source object — so
  the emitter follows the prim, ready for the P30.2
  simulation + billboard render. `ParticleSystem` / `particle_pattern` were
  already re-exported from `sl-client-bevy`, so there was no re-export gap.
  **Live-verified on aditi:** 9 in-view particle sources ingested with varied
  patterns (`0x01` DROP / `0x02` EXPLODE / `0x08` ANGLE_CONE), flags, burst
  rates and real texture ids, over 2134 tracked objects, no null-system false
  positives; clean build/clippy/tests and no OpenSim login regression (OpenSim's
  Default Region carries no particle content, so the source ingest is exercised
  on real SL).
- [x] **P30.2. Simulate + render.** A CPU particle simulation mirroring
  `LLViewerPartSim` / `LLViewerPartSourceScript` (emission patterns, wind,
  acceleration, interpolation) rendered as camera-facing billboards
  (`LLVOPartGroup`), textured via the texture pipeline. Net-new was an
  extension of the `particles` module: an `Emitter` (port of
  `LLViewerPartSourceScript::update` — the burst-timing accumulator, the
  angular-velocity source rotation, the `max_age` death, and the DROP /
  EXPLODE / ANGLE / ANGLE_CONE emission patterns, with a small deterministic
  xorshift RNG standing in for `ll_frand`), a `Particle::integrate` (port of
  `LLViewerPartGroup::updateParticles` — the velocity/accel Verlet step,
  `TARGET_POS` / `TARGET_LINEAR` attraction, `BOUNCE`, `FOLLOW_SRC` drift, and
  the colour / scale / glow interpolation), and `build_cloud_mesh` (port of
  `LLVOPartGroup::getGeometry` — a camera-facing quad per particle with the
  `FOLLOW_VELOCITY` re-orientation). The `drive_particles` system keeps one
  `ParticleSim` cloud per source: a dedicated **world-space entity** (identity
  transform, not a child of the source — mirroring `LLVOPartGroup` being its
  own spatial object) whose dynamic mesh is rebuilt each frame from the live
  particles, one `StandardMaterial` whose blend mode (additive vs alpha) and
  unlit-ness come from the system's blend func + `EMISSIVE` flag, and its
  texture pulled through the shared texture pipeline (or a procedural
  soft-sprite default, the `sDefaultParticleImagep` counterpart, when the
  source names none). The sim runs in Bevy world space; emission directions
  are built in Second Life space and carried over by the single basis change,
  with the source's SL-space rotation recovered from its Bevy
  `GlobalTransform`. Deliberate simplifications (documented in-module): region
  **wind** is not ingested (`WIND` is a no-op), the camera-distance rate
  **throttle** is not ported (only the hard 4096 particle cap), `RIBBON` /
  `BEAM` render as ordinary billboards, and a `TARGET_*` source falls back to
  its own position (the reference's own fallback). Two cross-cutting facts
  worth recording: (1) the cloud entity needs **`NoFrustumCulling`** — Bevy
  computes a mesh's `Aabb` once when `Mesh3d` is added (from the then-empty
  mesh), so a per-frame-rebuilt cloud is otherwise culled from every viewpoint
  (the same reason `objects.rs` opts its rebuilt meshes out); (2) a debug
  affordance `SL_VIEWER_PARTICLE_FOCUS=1` snaps the fly-camera to look at the
  busiest particle cloud, so an unattended screenshot can frame a real emitter
  without hand-aiming. **Live-verified on aditi:** a fountain's upward jets
  render as continuous streams of camera-facing billboards (not brief flashes),
  ~2700 live particles across 28 sources spanning DROP / EXPLODE / ANGLE_CONE
  patterns; clean build/clippy and 16 new unit tests over the RNG, emitter,
  integrator, and mesh builder. As with P30.1, OpenSim's Default Region carries
  no particle content, so the render is exercised on real SL.

## Phase 31 — General physics foundation (`avian3d`)

Flexi prims (Phase 32) and avatar body physics (Phase 34) are client-side
simulations. Rather than hand-rolling a solver for each, stand up a shared
physics substrate on the `avian3d` Bevy physics engine first.

### Simulator authority & the Firestorm motion model (read before P31.2)

Object **and avatar** position is *entirely* simulator-authoritative — the
reference viewer never runs a client-side physics solve for their placement, and
**does no collision/wall prediction** (not even for the own avatar; the agent
body is the same `LLViewerObject` path). It only **dead-reckons** between
updates from the sim-sent linear velocity + acceleration
(`LLViewerObject::interpolateLinearMotion`, called from `idleUpdate`):
`new_pos = (vel + 0.5*(dt - PHYSICS_TIMESTEP)*accel) * dt`. No geometry is
consulted. The load-bearing protocol contract (verbatim from that function): the
sim *"will NOT send updates if the object continues normally on the path
predicted by the velocity and the acceleration (often gravity) sent to the
viewer"* — so silence means "prediction still holds", and a deviation (a wall, a
push, a settle, a script stop) is communicated by a **corrective update**, not
foreseen by the client. During the round-trip the viewer genuinely extrapolates
slightly *into* the wall and is then snapped back. There is **no** "settled"
flag; rest is inferred from a terse update carrying ~zero linear/angular
velocity.

Because unbounded extrapolation "walks off into infinity" (and sinks avatars
under the terrain / shoots them off on region crossings), the reference bounds
the dead-reckoning with a layered set of guards that P31.2's smoothing step
**must reproduce** rather than let a body free-run:

- **Circuit-health phase-out.** After `sPhaseOutUpdateInterpolationTime` (2 s)
  of silence *and* a blocked/stale circuit (`LLCircuitData::isBlocked` / no
  packets
  — checked on the whole circuit, since per-object silence is expected), a
  `phase_out` factor ramps `1.0 → 0.0`, multiplying both the position delta and
  velocity so the object **eases to a halt**; by `sMaxUpdateInterpolationTime`
  (3 s) prediction is fully off. The circuit gate is essential: it separates
  "quiet because the prediction is right" (keep going) from "quiet because the
  sim is lagging" (taper off).
- **Geometric clamps.** Each extrapolated step is clamped to a **ground floor**
  (avatars use a real land-height lookup `resolveLandHeightGlobal + 0.5*height`
  so a laggy avatar does not dead-reckon under the terrain), a
  **region-height ceiling**, and an **off-region edge clip**
  (`clipToVisibleRegions`) that, when the predicted position leaves into a void
  with no neighbour, clips to the edge,
  **zeros velocity + acceleration, and waits for a server update**.
- **Region-crossing cap.** A tighter `sMaxRegionCrossingInterpolationTime` (1 s)
  bounds interpolation across a border crossing (the classic "shot off across
  the region" source).

Implications for the implementation phases, to stay faithful:

- **Keep server-driven prims *and* avatars kinematic** — driven by
  `ObjectUpdate` transforms with, at most, this velocity+accel dead-reckoning
  (the "avian smooths between updates" half of P31.2), *including* the phase-out
  and clamps above. Do **not** integrate them as free dynamic bodies under the
  configured gravity: the moment a server object free-runs, the "sim considers
  it settled (and goes silent) but avian keeps simulating" divergence appears,
  with no incoming update to correct it — the one case the corrective-update
  model cannot close. avian's genuine *dynamic* bodies + the world `Gravity` are
  for **client-only** motion the sim never simulates (Phase 32 / 34), not for
  re-simulating server objects.
- **Client-only physics self-settles, so it has no authority conflict.** Flexi
  (Phase 32) and the avatar cloth/body params (Phase 34) are spring-damper
  systems driven by the sim/animation-authored motion; with zero input they
  relax to their rest morph rather than running away, so they cannot "un-settle"
  a settled avatar/prim the way a gravity-driven rigid body would.

The viewer today does **not** even dead-reckon — `objects.rs` snaps each
transform straight to the last reported `object.motion.position`. So adding the
Firestorm velocity extrapolation (with the guards above) is itself part of the
P31.2 "smooths between updates" work, not a prerequisite already in place.

- [x] **P31.1. Integrate `avian3d`.** Add the `avian3d` plugin: a physics
  world with SL gravity, a fixed timestep, and coordinate bridging to the Y-up
  scene. Foundation reused by Phase 32 and Phase 34. New workspace dependency.
  **Done:** `avian3d` `0.7.0` (its `bevy ^0.19` requirement matches the
  workspace Bevy) is added to `sl-client-bevy-viewer` only — like the render
  materials and the other viewer-only simulations (sky / water / particles) the
  physics world is a viewer rendering concern, not a protocol capability, so the
  runtime-parity rule does not apply and `sl-client-tokio` gets nothing. A new
  viewer `physics` module owns a `PhysicsPlugin` that adds
  `PhysicsPlugins::default()` and configures the three foundation pieces: (a)
  **gravity** — Second Life's `-9.8` m/s² Z-up world gravity (Firestorm
  `llmath.h` `GRAVITY`, OpenSim `world_gravityz`) carried through the single
  Second Life → Bevy basis change (`coords::sl_to_bevy_vec`), so avian's
  `Gravity` resource points along Bevy `-Y`; (b) **fixed timestep** — avian runs
  its schedule in `FixedPostUpdate` driven by Bevy's `Time<Fixed>`, pinned to
  the simulator's target physics rate `SL_PHYSICS_HZ = 45`; (c) **time
  dilation** —
  avian's physics-clock *relative speed* (its own docs call it "time dilation")
  is set each frame from the agent region's `RegionData.TimeDilation` (already
  surfaced as `Event::TimeDilation`, folded per-region into a
  `RegionTimeDilation` resource and looked up by
  `SlIdentity::region_handle`), so client-side dynamics slow in lock-step with a
  laden sim instead of drifting
  ahead of it, defaulting to full speed while the region is unknown / healthy.
  The physics world is empty (no bodies) until P31.2 gives server-flagged prims
  rigid bodies, so there is no visible change yet. Verified: clean
  build/clippy + 3 unit tests (gravity axis map, dilation clamp, bad-value
  guard) and an OpenSim login smoke run (region handshake + clean quit, no
  panics / avian / schedule errors).
- [x] **P31.2. Physical objects.** Give server-flagged physical prims (the
  `LLViewerObject` physics flag / `LLPhysicsShapeType` — prim / convex hull /
  none) an avian rigid body + collider derived from the prim / mesh geometry.
  The sim stays authoritative — `ObjectUpdate` transforms drive the body while
  avian smooths between updates and powers client-only dynamics. **Follow the
  "Simulator authority & the Firestorm motion model" note above:** drive these
  bodies **kinematically** (transform from `ObjectUpdate` + velocity/accel
  dead-reckoning with the circuit-health phase-out and the ground /
  region-height / off-region-edge clamps), **not** as free dynamic bodies under
  the world gravity — otherwise a server object the sim has settled (and gone
  silent about) keeps free-running in avian with no update to correct it.
  Reserve avian's dynamic bodies for genuinely client-only motion
  (Phases 32 / 34). **Done:** `objects.rs`'s `apply_object` now stamps every
  server-flagged physical **root** prim (`FLAGS_USE_PHYSICS`, non-attachment —
  attachments follow their wearer's joint, and linkset children ride the Bevy
  hierarchy) with a `PhysicalObject` marker (the `apply_light` /
  `apply_particles` insert-or-remove pattern), change-detected so a fresh insert
  on every update reseeds. From that marker `physics.rs`'s
  `drive_physical_objects` attaches a **kinematic** avian `RigidBody` + a
  `Collider::cuboid` sized to the prim scale (rebuilt only on a genuine resize),
  snaps the body to each authoritative update, and between updates dead-reckons
  the pose forward as a faithful port of
  `LLViewerObject::interpolateLinearMotion`: the
  `(vel + 0.5*(dt - PHYSICS_TIMESTEP)*accel) * dt` extrapolation (scaled by
  region time dilation, the reference's `idleUpdate`
  `dt = time_dilation * dt_raw`), the `applyAngularVelocity` spin, the
  circuit-health **phase-out** (ramps `1 → 0` between 2 s and 3 s of silence
  *only once the circuit looks stalled* — a new `CircuitLiveness` resource
  tracking the last inbound event stands in for `LLCircuitData::isBlocked` / the
  last-packet time, so "quiet because prediction holds" keeps going while "quiet
  because the sim lags" eases to a halt), and the geometric clamps
  (region-height ceiling, a permissive `getMinAllowedZ` ground floor from a new
  `TerrainState::land_height` land lookup, and the off-region-edge clip /
  region-crossing cap that zero velocity when a prediction would leave into a
  void vs. a known neighbour — neighbours read from the time-dilation-seen
  region set, the `clipToVisibleRegions` analogue). Kept viewer-only (no
  runtime-parity obligation, like the P31.1 world). The whole extrapolation is
  per-component `f32` / `Quat`-method math to satisfy the workspace
  `arithmetic_side_effects` lint. Verified: clean build/clippy + 12 unit tests
  (dead-reckon formula, phase-out ramp/gating, angular step, the ceiling /
  floor / void-clip / region-crossing clamps, ground-floor radius, neighbour
  lookup) and a **live OpenSim** run — a 1 m physical box dropped mid-session (a
  `<Flags>Physics</Flags>` OAR merge-loaded while the viewer was already
  streaming, so it fell live under the region's `ubODE` engine) was received
  flagged physical, given a `1.00×1.00×1.00 m` kinematic body, and dead-reckoned
  through its fall onto the avatar (user-confirmed on screen), with a clean quit
  and no panics / avian / schedule errors. Two aspects are deliberately deferred
  to their own points below: the CAPS `LLPhysicsShapeType` (prim / hull / none)
  and a real geometry-derived collider (the P31.2 collider is a scale-sized
  cuboid regardless) → **P31.3**; and dead-reckoning of **avatars** (a separate
  `avatars.rs` path) → **P31.4**.
- [x] **P31.3. Physics-shape-aware colliders.** Replace P31.2's placeholder
  scale-sized cuboid with a collider that matches the object's real
  `LLPhysicsShapeType` and geometry. Fetch it from the CAPS
  `GetObjectPhysicsData` (`ObjectPhysicsProperties`, surfaced as
  `Event::ObjectPhysicsProperties`): **none** → no collider (a physics prim with
  no collision shape); **convex hull** → an avian convex hull from the prim /
  mesh vertices; **prim** → the tessellated prim / mesh geometry (or its convex
  decomposition). Uses avian's `collider-from-mesh` (already a default feature)
  over the geometry the viewer already tessellates. Matters once P32 / P34 add
  genuine dynamic bodies that collide against these kinematic movers — until
  then the cuboid is inert. **Done:** the whole
  wire/proto/runtime stack for the physics data already existed (`sl-wire`'s
  `object_physics` — the `PhysicsShapeType` (none / prim / convex-hull) +
  `ObjectPhysicsData` types and the `GetObjectPhysicsData` LLSD codecs — plus
  `Command::RequestObjectPhysicsData`, both `Event::ObjectPhysicsData` (cap
  reply, keyed by full `ObjectKey`) and `Event::ObjectPhysicsProperties`
  (event-queue push, keyed by `ScopedObjectId`), and
  `CAP_GET_OBJECT_PHYSICS_DATA`, all wired through both runtimes), so the only
  net-new library change was re-exporting `PhysicsShapeType` +
  `ObjectPhysicsData` from **both** `sl-client-bevy` and `sl-client-tokio` (a
  latent parity gap — only the sim-features `PhysicsShapeTypes` plural was
  exported). In the viewer, `physics.rs` gained a `full_key` on
  `PhysicalObject`, an `ObjectPhysicsShapes` resource, and three systems.
  `request_object_physics_data` fires one `RequestObjectPhysicsData` per object
  the first time it is flagged physical (guarded by a `requested` set — the
  reliable path, since a grid only *pushes* `ObjectPhysicsProperties` on a
  physics-material change, not on stream-in). `ingest_object_physics_data` folds
  both delivery paths into the resource by full key (translating the push's
  `ScopedObjectId` via a new `ObjectState::full_key`). And
  `refine_physical_colliders` builds the shape-aware collider once the data and
  geometry are both available: **none** removes the collider, **convex hull** is
  a `Collider::convex_hull` of the object's own vertices, **prim** (or an
  unrecognised type) is a `Collider::trimesh` of that geometry. The geometry is
  gathered from the object's own faces via a new `GeometryHolder` marker on the
  per-object geometry holder (so linkset child prims — which also parent to the
  object entity — are excluded), each vertex scaled by the object scale into the
  object-root entity's local frame (where the collider lives; the entity
  `Transform` carries the basis change, matching how the same faces render
  through the holder scale — the points are **not** pre-basis-changed). Collider
  ownership moved entirely to `refine_physical_colliders`:
  `drive_physical_objects` (P31.2) now only seeds the initial placeholder
  cuboid, and a new `RefinedCollider { shape, from_geometry, scale }` records
  what was built so a collider is rebuilt only on a real change (new shape data,
  a resize, or the geometry finally uploading — retried each frame until then).
  These colliders are inert on the kinematic movers themselves, so verification
  is log-based. Verified live on OpenSim with two throwaway
  `<Flags>Physics</Flags>` box OARs whose `<PhysicsShapeType>` the OAR
  serializer round-trips (`bin/slclient-physics.oar` shape 2 / convex-hull,
  `bin/slclient-physics-prim.oar` shape 0 / prim): each 1 m cube was requested,
  its shape received, and refined to the matching collider from its 24
  vertices — `ConvexHull collider from 24 vertices` and `Prim collider from 24
  vertices` — with a clean quit and no panics / avian / wgpu errors. Six new
  unit tests (shape-needs-geometry, resize detection, index-offset triangle
  merging,
  and building a convex hull / a trimesh from a cube).
- [x] **P31.4. Avatar dead-reckoning.** Extend the P31.2
  `interpolateLinearMotion` port to the own and other avatars (the `avatars.rs`
  path, not the object path), so a laggy avatar dead-reckons from its sim-sent
  velocity / acceleration with the same phase-out and clamps. Avatars use the
  stricter **ground floor** the reference viewer applies to them
  (`resolveLandHeightGlobal + 0.5*height` via `TerrainState::land_height`) so a
  laggy avatar does not sink under the terrain — the one guard P31.2 left
  permissive for objects. Keep avatars **kinematic** (sim-authoritative), like
  the objects. **Done:** the P31.2 object dead-reckoner was refactored so
  its extrapolation core is shared — a new `MotionState` (the evolving
  predicted pose + motion, all in Second Life space) plus an
  `advance_motion` step (the dead-reckon + geometric-clamp + angular-spin,
  taking a caller-supplied ground floor) now back **both** paths;
  `PhysicsInterp` was reshaped to hold a `MotionState`, unchanged in
  behaviour. On the avatar side, `apply_object` (`avatars.rs`) stamps each
  full-object avatar's anchor with a new `AvatarMotion` (change-detected,
  re-inserted every update — a rigged body root carries the object
  rotation, a placeholder sphere does not), and a new `drive_avatar_motion`
  system dead-reckons it with the **stricter avatar ground floor**
  (`avatar_ground_floor` = `land + 0.5*height`, vs. the object floor's
  permissive `land - radius`). Because the anchor's Bevy `Transform` also
  carries the pelvis / shoe vertical render offset (owned by `apply_object`
  / the appearance path), the driver moves it by the SL-space position
  *delta* (the basis change is linear, so it converts directly) rather than
  recomputing it absolutely, leaving that offset intact; on an
  authoritative update `apply_object` has already snapped the anchor to
  truth, so the driver only reseeds. Kept viewer-only (no runtime-parity
  obligation). Eight new unit tests (avatar floor, the shared
  `advance_motion` dead-reckoning + floor clamp + the still-body no-op, plus
  the movement-control helpers below) on top of the P31.2 suite. Verified
  live on OpenSim: the own rigged avatar was seeded (`avatar … →
  dead-reckoned (height 1.90 m, rotates true)`) and — driven by the new
  movement controls (P31.5) — walked / turned / flew **smoothly** between
  the sim's updates (user-confirmed on screen), with a clean session and no
  panics / avian / schedule errors. **Follow-up noted:** the avatar does
  not yet *animate* while it moves → **P31.6**.
- [x] **P31.5. Avatar movement controls.** A prerequisite that rode along
  with P31.4: a fly-camera-only viewer never moves the own avatar, so
  avatar dead-reckoning could not be *observed* (the stationary own avatar
  reseeds to truth every frame). Second Life avatar motion is entirely
  simulator-authoritative — the client advertises *intent* in `AgentUpdate`
  (`ControlFlags` + the body facing the walk direction follows) and the sim
  moves the avatar and streams it back — so this drives that intent from
  the keyboard, which is exactly what feeds the P31.4 dead-reckoner. A new
  `movement.rs` (viewer-only) reads keys that do **not** clash with the WASD
  and mouse fly-camera: **↑ / ↓** walk forward / back (`AT_POS` / `AT_NEG`),
  **← / →** turn the client-tracked heading (sent as the `AgentUpdate` body
  rotation the walk follows, seeded once from the own avatar's reported
  facing so the first step does not snap), **PageUp / PageDown** ascend /
  descend, **F** toggle fly, **Shift + ↑ / ↓** run (`FAST_AT`). No stop key
  — the flag set is recomputed from the held keys each frame, so releasing a
  key drops its flag. It emits a command only when the intent *changes* (a
  `SetControls` on a flag change, a throttled `SetRotation` while turning),
  relying on the session's keep-alive re-send of the held controls. The
  whole movement / rotation stack (`Command::SetControls` / `SetRotation`,
  `ControlFlags`) already existed end-to-end through both runtimes, so this
  was a viewer-only addition (module + registration). Verified in the same
  live run above.
- [x] **P31.6. Locomotion / state animations.** Drive the avatar's
  built-in state animations — the ones an animation overrider (AO) replaces
  — from its movement state: **standing**, **walking** / **running**,
  **turning left / right**, **flying** / **hovering**, **falling**,
  **jumping**, **crouching**, **sitting**. On the wire the simulator drives
  these: it plays a state animation on the avatar (e.g. `ANIM_AGENT_WALK`)
  and pushes the set via `AvatarAnimation`, which the viewer already ingests
  and plays through the Phase 18 pipeline. That pipeline is already proven —
  on **aditi** the viewer has played the sim's standing animations in past
  runs — so the first task is to find why the **local OpenSim** P31.4/P31.5
  run showed no locomotion animation on the moving avatar: whether OpenSim
  drives `AvatarAnimation` for these states at all, and whether the received
  set is reaching the play path for the own avatar. Only then add a
  **client-side** fallback (derive the state from the P31.4 velocity / the
  movement `ControlFlags` and play the corresponding built-in `.anim` from
  the `character/` set) for the own avatar's immediate feedback / where the
  sim does not drive it. Reference: Firestorm `LLAgent` motion controllers
  and `llvoavatar` `ANIM_AGENT_*` ids. **Done — the investigation found the
  planned premise wrong in two ways, so the fix is two library bug-fixes plus
  a viewer fallback, not the assumed synthesis / local `.anim`.** (1) **Root
  cause: an `sl-anim` registry misclassification.** walk / run / stand /
  turn / crouch (and the female / `*_new` / `standup` variants) were marked
  `BuiltinKind::Procedural` (no downloadable asset), so the resolver skipped
  the fetch and *never played them*. But the reference viewer's
  `LLKeyframeWalkMotion` / `LLKeyframeStandMotion` / `LLKeyframeFallMotion`
  all **extend `LLKeyframeMotion`**, which downloads the keyframe asset by
  UUID (`gAssetStorage->getAssetData`) and only layers a procedural
  *adjustment* (foot IK / torso facing) on top — the assets are ordinary
  downloadable `.anim`s (confirmed: OpenSim serves them under the exact
  built-in UUIDs, e.g. `walk` `6ed24bd8…`). Fixed by reclassifying the 17
  locomotion entries `Procedural → Keyframe`; the genuinely procedural ones
  (the `LLEmote` `express_*`, `do_not_disturb`, and the always-on adjusters —
  which the sim never signals and are absent from the table) stay
  `Procedural`. Added `sl_anim::builtin_animation_by_name` (name → built-in)
  for the viewer's state → UUID lookup. So there is **no local `.anim` from
  `character/`** and **no synthesis** — the built-ins download from the grid
  like any other, and the sim-driven path (the primary one, per the note
  above) then works. (2) **A second, latent P18.4 bug the fix exposed:**
  `reconcile_playing` stamped a dropped animation's `stopped_at` with the
  **absolute wall clock** (`now`), while `Motion::pose_weight` /
  `is_finished` compare it against `elapsed = now - start` (**motion-elapsed**
  since that animation started). A *non-looping* motion was saved by its
  natural ease-out (`min(stopped_at, duration - ease_out)` picks the smaller),
  so gestures always faded correctly and the bug stayed hidden — but a
  *looping* locomotion motion has no natural ease-out, so a stopped walk held
  full weight until `elapsed` reached the (large, ever-growing) wall-clock
  value: it "stuck" on walk for a few seconds early in a session and
  effectively forever later. Fixed by storing `stopped_at = now - start`
  (the documented motion-elapsed timeline); regression-tested. (3)
  **Client-side fallback (`locomotion.rs`, viewer-only):** derives the own
  avatar's state from the P31.5 advertised `ControlFlags` **intent** (walk /
  run / turn / fly / ascend / descend) plus the P31.4 dead-reckoned
  *vertical* velocity (fall / fly-vertical only), maps it to the built-in
  animation via `builtin_animation_by_name`, and plays it on a new
  client-driven slot on `AnimationPlayback` — **deferring entirely** while the
  sim is driving the avatar (a new `has_active_sim_animation` gate), so the
  two never double-drive. Intent (not velocity) drives walk/run/turn on
  purpose: the intent clears the instant the key is released, whereas the
  last-reported velocity lingers at walk speed until a corrective update — the
  same "doesn't stop when you stop" trap in miniature. Diagnostics:
  `SL_VIEWER_LOG_LOCOMOTION=1` (edge-logged state + the sim's per-update
  `AvatarAnimation` set) and `SL_VIEWER_FORCE_CLIENT_LOCOMOTION=1` (force the
  fallback on to exercise it on a root presence). Kept viewer-only (no
  runtime-parity obligation); `sl-anim` is the only shared-library change.
  Verified **live on OpenSim** (user-confirmed on screen): walk / stand fetch,
  decode, and pose the skeleton; the own avatar walks and settles back to
  standing **promptly** on key release (the wire log shows OpenSim broadcasts
  clean `walk#n ↔ stand#n+1` transitions and the reconcile now eases the walk
  out within its ~0.5 s ease-out); this login was a root presence so the sim
  drove it and the client fallback correctly stayed deferred. **Scope: only the
  base keyframe motion plays.** The reference viewer's *procedural adjustment*
  overlay — the whole point of the `LLKeyframe*Motion` subclasses — is **not**
  ported: `LLKeyframeWalkMotion`'s playback-speed match to ground velocity +
  `LLWalkAdjustMotion` foot-plant IK / pelvis lag (anti-foot-skate),
  `LLKeyframeStandMotion`'s lower-body twist to face the look direction with
  foot IK, `LLKeyframeFallMotion`'s landing recovery, and the always-on
  adjusters the sim never signals (`LLHeadRotMotion` head-track, `LLEyeMotion`,
  `LLHandMotion`, `LLBodyNoiseMotion` idle sway, `LLBreatheMotionRot`). So feet
  can skate, the stand does not turn its legs to the look direction, and there
  is no head/eye/breathe idle motion → **P31.8**. **Follow-up noted:** avatar
  turning is not interpolated like translation, so it reads choppy → **P31.7**
  (a motion-smoothing gap, unrelated to the animations).
- [x] **P31.7. Interpolate avatar turning.** The own avatar's **rotation** is
  not smoothed the way its **translation** is (P31.4): a live finding from the
  P31.6 run is that turning left / right reads choppy while forward / backward
  motion is smooth. The P31.4 avatar dead-reckoner (`drive_avatar_motion`)
  advances position from the sim-sent linear velocity + acceleration and spins
  the orientation from the angular velocity, but the own avatar's facing is
  *driven client-side* by the P31.5 movement controls (a throttled
  `SetRotation` at ~20 Hz seeds the sim, which streams the resulting facing
  back as terse `ObjectUpdate`s), so between those sparse updates the anchor's
  rotation snaps rather than interpolating. Smooth the avatar's orientation
  between authoritative rotation updates (slerp toward the target facing, or
  fold the client-tracked heading into the render transform continuously) so a
  turn looks as fluid as a walk. Viewer-only; unrelated to the P31.6
  animations. Reference: Firestorm `LLViewerObject::interpolateRotation` /
  the agent's `mDrawable` orientation smoothing. **Done — viewer-only, in
  `physics.rs` (`drive_avatar_motion` / `AvatarInterp`).** Root cause matched
  the premise: the own avatar's facing arrives only as sparse `ObjectUpdate`s
  echoing the client-driven `SetRotation` (essentially zero angular velocity,
  so the dead-reckoner's `angular_step` never advanced it between updates), and
  both the update-frame snap (`apply_object` writing `body_root_transform`) and
  the between-update path wrote the *authoritative* facing straight onto the
  anchor — so the rotation stepped while translation eased. Fix: `AvatarInterp`
  now carries a `rendered_rotation` (Bevy space) that each frame **slerps**
  toward the current authoritative / dead-reckoned facing
  (`apply_smoothed_rotation`) with a framerate-independent exponential blend
  (`rotation_smoothing_alpha`, `1 - e^(-dt/τ)`, τ = 80 ms) instead of snapping;
  the reseed and both dead-reckon exit paths route through it, so the
  smoothing spans the update boundary. Chosen over folding the client-tracked
  heading in because it is general (smooths **every** avatar's turning, not
  just the own one) and needs no cross-module coupling to `movement.rs`. τ =
  80 ms converges to the target and leaves no standing lag once turning stops,
  so a stationary facing is still exact. Unit-tested (`rotation_smoothing_alpha`
  easing curve + slerp convergence-without-snap); the base transform snap was
  never the visible artifact so no regression there. **Verified live on
  OpenSim** (user-confirmed on screen): ← / → turning now reads as fluid as
  the ↑ / ↓ walk. Only the *base* facing is smoothed — the reference viewer's
  `LLKeyframeStandMotion` lower-body twist to the look direction is still
  P31.8.
- [x] **P31.8. Procedural motion adjustments & always-on adjusters.** P31.6
  plays each state's *base* downloadable keyframe (walk / run / stand / turn /
  fall), but not the procedural layer the reference viewer stacks on top —
  which is the whole reason those states are `LLKeyframe*Motion` **subclasses**
  rather than plain `LLKeyframeMotion`. Port them as post-keyframe pose
  adjustments over the Phase 18 blend (they run every frame after the sampled
  pose, driven by agent state, not by an `AvatarAnimation`):
  - **Locomotion adjustments** — `LLKeyframeWalkMotion` matches the walk/run
    playback speed to the actual ground velocity, and the always-on
    `LLWalkAdjustMotion` foot-plants with pelvis lag, together killing
    foot-skate; `LLKeyframeStandMotion` twists the lower body / feet to face
    the look direction with foot IK (so a standing avatar's legs follow the
    camera); `LLKeyframeFallMotion` blends the landing recovery;
    `LLFlyAdjustMotion` banks the fly.
  - **Always-on idle adjusters** — `LLHeadRotMotion` (head / neck tracks the
    look-at target), `LLEyeMotion` (eye saccades / tracking), `LLHandMotion`
    (hand-pose morph), `LLBodyNoiseMotion` (subtle idle sway), and
    `LLBreatheMotionRot` (chest breathing). The viewer runs these continuously
    on every avatar; they are **not** signalled over `AvatarAnimation` and so
    are absent from `sl-anim`'s table.
  - **Activity-driven reach / aim** — `LLEditingMotion` reaches the hand toward
    the object the agent has selected / is editing; `LLTargetingMotion` aims the
    arm at a target (mouselook aim / the point gesture). Also locally driven,
    also absent from the table.

  (`LLPhysicsMotionController` avatar body-jiggle physics is separate — Phase
  34.) Reference: `llkeyframewalkmotion.cpp` / `llkeyframestandmotion.cpp` /
  `llkeyframefallmotion.cpp` and the adjuster motion classes in
  `indra/newview/`; the `registerMotion` block in `llvoavatar.cpp`. **Done — the
  two always-on idle adjusters that need no external input landed; every other
  adjuster listed above is deferred to its own item (P31.12–P31.15) because it
  needs state this pass has no access to.** New `procedural.rs` (viewer-only,
  like the P31.6 locomotion fallback) ports the input-free always-on pair as
  pure, unit-tested functions folded into `pose_avatar_skeletons` as a
  post-keyframe pass over the resolved [`AnimationPose`]: **breathe**
  (`LLBreatheMotionRot`) — a slow `sin(time)·0.05` pitch of `mChest` about its
  local Y, an exact port; and **body noise** (`LLBodyNoiseMotion`) — a subtle
  ≤1° low-frequency sway of `mTorso` about local X/Y at `TORSO_NOISE_SPEED`.
  Each is composed as a small delta *on top of* whatever the keyframe pose
  already animates for that joint (`base·delta`), so a playing animation still
  dominates and the idle motion only shows where the joint is otherwise at rest
  — the effect the reference gets from these motions' additive / low-priority
  blend. Applied to every rigged avatar each frame, so an idle avatar breathes
  and sways instead of standing frozen. Body noise is faithful in *character*
  but not a bit-for-bit Perlin port: the reference `noise2` tables are
  `llrand`-seeded every viewer startup, so there is no canonical waveform to
  match, and a full port would also need a lint-scoped `as` on the one float→
  lattice-index truncation (Rust std has no `From`/`TryFrom` for float→int); a
  cast-free incommensurate-sine noise reproduces the slow wander for less code.
  Not ported (each gates its motion by avatar pixel area — not modelled
  here, the pose pass already runs only for rigged avatars): head/eye look-at
  tracking, hand-pose morph, the IK locomotion adjustments, and the reach/aim
  motions → **P31.12–P31.15**. Library change: `AnimationPose::{rotation,
  position}` made `pub` in `sl-client-bevy` for the viewer to read-modify-write
  a joint's resolved rotation. Verified: unit tests (breathe rest/peak, sway
  amplitude bound, sway moves over time, delta-composes-on-keyframe-base, absent
  joints skipped); build + clippy clean.
- [ ] **P31.9. Typing animation & sound.** When an avatar types in nearby
  chat the reference viewer plays `ANIM_AGENT_TYPE` (the hands-on-keyboard
  gesture) plus the typing UI sound, and advertises the state so others see
  it. For **other** avatars this should already flow through the P31.6-fixed
  path — the simulator plays `ANIM_AGENT_TYPE` (a downloadable keyframe, now
  correctly classified) and broadcasts it over `AvatarAnimation`, which Phase
  18 plays — so the first task is to **verify** a typing neighbour animates
  (a second `sl-repl` login sending `StartTyping`). For the **own** avatar,
  drive it from local chat-entry state: on start-typing play the animation
  locally *and* send `ChatFromViewer` `StartTyping` (the `ChatTyping` P11.1
  already ingests for others), clear it on stop-typing; optionally the typing
  sound. A sibling of P31.6 — an activity-driven state animation, not a
  procedural adjuster. Reference: `LLAgent::startTyping` / `stopTyping`,
  `ANIM_AGENT_TYPE`, `gAgent.sendAnimationRequest`.
- [ ] **P31.10. Voice lip-sync — deliberately OUT OF SCOPE (recorded so it is
  a known gap, not an oversight).** The reference viewer animates an avatar's
  mouth from the live voice **audio power** while it speaks
  (`LLVoiceVisualizer` — a viseme / mouth-open morph driven by the speaker's
  amplitude), plus the green voice-dots "who's speaking" indicator. Both need
  the decoded voice **audio stream**, which sl-client does not carry: the
  project models voice **signalling / session-state only**, not the
  Vivox / WebRTC audio transport, and the speaking indicators are out too (see
  the voice-signalling-only decision in the sl-client memory). So there is
  nothing to drive lips or dots from. Left unchecked as a permanent,
  intentional boundary; revisit only if voice audio is ever brought in scope.
- [ ] **P31.11. Auto-stop flying on landing.** In the reference viewer, flying
  down to the ground automatically turns flight off — once the avatar touches
  (or nears) the ground / a surface, the viewer clears the fly state so the
  agent lands and stands rather than hovering just above the ground still in
  fly mode. Our viewer's P31.5 fly toggle (`movement.rs`, **F**) never clears
  itself: it stays `flying = true` until the user presses **F** again, so a
  descent to the ground leaves the avatar stuck in a hovering fly state. Detect
  the landing — the reference path is `LLAgent::setFlying(FALSE)` driven from
  the fly state machine (`FS_STATE`/`fly` handling in `llagent.cpp`) when the
  agent is on / very close to the ground while descending (roughly: fly is on,
  `UP_NEG`/no lift, and the dead-reckoned height is at the ground floor), and
  the sim itself also stops broadcasting the fly animation — and clear
  `AvatarControls::flying` (dropping `ControlFlags::FLY` from the advertised
  intent, which also lets the P31.6 locomotion fallback fall back out of the
  fly / hover states). Keep the manual **F** toggle for taking off again.
  Viewer-only (movement / control-flag plumbing already exists end-to-end).
  Reference: `LLAgent::setFlying` / the ground-detect in `llagent.cpp`.
- [ ] **P31.12. Head & eye look-at tracking (`LLHeadRotMotion` /
  `LLEyeMotion`).** The always-on adjusters split out of P31.8 that need a world
  **look-at target**, which the viewer does not yet track. `LLHeadRotMotion`
  turns the head toward the target and lags the neck (`NECK_LAG`) and torso
  (`TORSO_LAG`) behind it, constrained to `HEAD_ROTATION_CONSTRAINT`; with no
  target it faces the root forward (rest), so it is a near no-op until a target
  exists. `LLEyeMotion` aims the eyes at the target with vergence, layers random
  saccades / look-away jitter on top (bounded by `EYE_ROT_LIMIT_ANGLE`), and
  blinks by driving the `Blink_Left` / `Blink_Right` **morph visual-params**
  (needs runtime per-frame visual-param morphs, which the appearance pipeline
  bakes once — an extra prerequisite). First provide the look-at target: for the
  own avatar derive it from the camera / cursor focus; for others from the
  sim-relayed `LookAt` (the `ViewerEffect` look-at the P11-era data carries).
  Then port head/neck/torso lag and the eye aim + saccades. Reference:
  `llheadrotmotion.cpp` (`LLHeadRotMotion` / `LLEyeMotion`).
- [ ] **P31.13. Hand-pose morph (`LLHandMotion`).** The always-on adjuster split
  out of P31.8 that morphs the hand between the named hand-pose animations (the
  `HandPose` a `.anim` header carries, P18.1), cross-fading when the pose
  changes. Needs the hand-pose animation assets and a morph/blend between two
  keyframe hand poses. Reference: `LLHandMotion` in `indra/newview/`.
- [ ] **P31.14. Locomotion IK adjustments.** The locomotion group split out of
  P31.8 — every piece needs an **inverse-kinematics solver** the project does
  not have. `LLKeyframeWalkMotion` matches the walk/run playback speed to the
  ground velocity, and the always-on `LLWalkAdjustMotion` foot-plants with
  pelvis lag to kill foot-skate; `LLKeyframeStandMotion` twists the lower body /
  feet to face the look direction with foot IK (a standing avatar's legs follow
  the camera); `LLKeyframeFallMotion` blends the landing recovery;
  `LLFlyAdjustMotion` banks the fly. Build a small foot / limb IK pass first,
  then layer these over the P18 blend. Reference: `llkeyframewalkmotion.cpp` /
  `llkeyframestandmotion.cpp` / `llkeyframefallmotion.cpp`.
- [ ] **P31.15. Activity-driven reach / aim (`LLEditingMotion` /
  `LLTargetingMotion`).** The activity group split out of P31.8, needing
  selection / target state the viewer does not track. `LLEditingMotion` reaches
  the hand toward the object the agent has selected / is editing;
  `LLTargetingMotion` aims the arm at a target (mouselook aim / the point
  gesture). Both are locally driven and want a limb IK reach (shares the P31.14
  solver). Reference: the two motion classes in `indra/newview/`.

## Phase 32 — Flexi prims

- [ ] **P32.1. Ingest flexible-object data.** The `LLFlexibleObjectData` extra
  params (softness, gravity, drag, wind, tension, force). Bevy-free.
- [ ] **P32.2. Simulate.** Port the reference spring / chain deformation of
  the prim path over time (`LLVolumeImplFlexible` on `LLVOVolume`), built on
  the Phase 31 avian primitives where practical, deforming / re-tessellating
  the flexi geometry each frame.

## Phase 33 — Reflection probes

- [ ] **P33.1. Reflection-probe volumes.** Detect reflection-probe volumes
  (the prim reflection-probe flag / extra params — `LLReflectionMap`) and map
  them to Bevy light-probe / reflection-probe components, generating an
  environment cubemap per probe. Complements the Phase 27 PBR materials that
  sample them. Reference: `LLReflectionMapManager` / `RenderReflectionProbe`.

## Phase 34 — Avatar cloth & body physics

- [ ] **P34.1. Ingest the physics wearable.** The `WT_PHYSICS` wearable params
  — breast / belly / butt bounce driving params from `avatar_lad.xml`.
- [ ] **P34.2. Drive them.** Port `LLPhysicsMotion` /
  `LLPhysicsMotionController` (a spring-damper per param, driven by joint
  acceleration, built on the Phase 31 physics foundation) as a motion in the
  Phase 18 animation controller, folding the resulting param weights into the
  avatar morphs each frame. Reference: `llphysicsmotion.cpp`.

## Phase 35 — HUD attachments

- [ ] **P35.1. Detect HUD.** Classify an attachment whose `attachment_point()`
  is a HUD slot (31–38, `HudCenter` / `HudTopLeft` / …); route it out of the
  world scene to a dedicated screen-space HUD layer, and only for the **agent's
  own** attachments.
- [ ] **P35.2. HUD rendering.** Render HUD-attached prims/mesh on a HUD camera /
  render layer anchored per the HUD attachment-point screen layout (orthographic
  / screen-relative), reusing the existing prim/mesh geometry+texture build.
  Verify a simple HUD renders fixed to the screen on aditi.

## Phase 36 — Aditi (real SL) verification

OpenSim end-to-end and the clippy / fmt / `rumdl` clean sweep are **not** a
separate phase — they happen inside every phase above as it builds, live-tests,
and commits (per the Legend). What OpenSim can't exercise is the SL-only
appearance stack, so this final phase is the real-SL pass:

- [ ] **P36.1. Aditi (real SL).** Run against `credentials.aditi.toml` + the MFA
  wrapper for the SL-only paths OpenSim can't exercise: **server-side**-baked
  bodies (vs. OpenSim's client-side bake), BoM mesh bodies with alpha, the
  agent's HUDs, and the SL-heavy new phases — PBR materials / terrain, EEP
  environment + day cycle, and animesh.

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
- [x] **R11. Base-body mesh distorts under animation** — fixed by R13
  (`sl-avatar` / `sl-client-bevy`). Surfaced by P18.3: a *shaped* avatar's limbs
  (arms most visibly) stretch / distort while an animation plays, but look
  correct at rest and return to correct on stop. The **skeleton was posed
  correctly** all along — the joint world matrices are right and the bone
  lengths stay constant under animation (verified live from a per-frame
  `mShoulderLeft`→`mElbowLeft`→`mWristLeft` length dump: a steady `0.289` /
  `0.214` throughout dance1), so the distortion was in the **skin**, not the
  pose. The original premise here (that the base body needed the reference
  viewer's `LLSkinJoint` **pivot** scheme —
  `LLViewerJointMesh::uploadJointMatrices` baking `mRootToJointSkinOffset` /
  `mRootToParentJointSkinOffset` into the skinning matrix) was **disproven**:
  R12 measured the skin pivots as a sub-millimetre no-op, and R13 found the real
  cause — the base-mesh joint-render-data list was **including the extended
  (Bento) ancestors** (`mSpine*`) the reference viewer skips, shifting every
  weight index past them so whole arm chains bound to the wrong joint (invisible
  at bind pose, but a rest-pose armpit spike and gross arm distortion the moment
  a joint rotated). The R13 `base_ancestor` fix (skip non-base ancestors,
  `getBaseSkeletonAncestor` / SL-287) corrected the binding, and it was
  *expected* to also fix this animation-time distortion. **Re-checked and
  confirmed:** no new code was needed here. Verified live on the local OpenSim
  (own shaped avatar playing dance1 via `--play-animation`/`--repeat-animation`,
  offline screenshot harness, both head-on and a 50° orbit): across the full
  range of poses — elbows bent, arms spread wide sideways, arms raised — the
  limbs skin cleanly with no stretch, ballooning, or spikes. The arm distortion
  R11 describes is gone.
  Rigged-mesh bodies (Phase 17, ordinary skin weights) were never affected.
- [x] **R12. Own avatar renders bloated — publish/resolve the worn shape**
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
  **Fixed** — the "matching the worn shape" work `bake_publish.rs` had
  deferred: `OwnBakeInputs::visual_params` builds the transmitted vector from
  the worn wearables' params (a new `VisualParams::encode_appearance` +
  `f32_to_u8` quantizer, the inverse of `map_appearance`; a param no wearable
  sets falls back to its table default, so the vector is always the correct
  neutral Ruth shape, never the `128` midpoint). It is used for **both** the
  `AgentSetAppearance` publish (`drive_bake_publish`) and rendering the own
  avatar (`apply_own_shape_from_wearables`, which overrides the server-echoed
  appearance for our own agent and self-heals a re-outfit) — so the own avatar
  is correct on any account/grid regardless of server state and other viewers
  see the right shape. Verified live: a never-Firestorm'd account (Friend
  Tester) that stayed bloated now renders the correct slender shape a few
  seconds after login (once its wearables load). This was the *dominant*
  base-body appearance bug; the animation-time skin distortion (**R11**, whose
  skin-pivot premise turned out to be a proven sub-millimetre no-op) is a
  separate, smaller issue to tackle next. Two viewer debug affordances were
  added to make this comparison possible: `--screenshot-dir` (an offline PNG
  capture harness that quits after N frames) and `--repeat-animation` (keep
  re-issuing `--play-animation` so a short motion still plays once loaded).
- [x] **R13. Rest-visible spike under one shoulder** (`sl-client-bevy` /
  `sl-avatar`). With the shape correct (**R12**), a triangular flap of geometry
  poked out under the avatar's **right** armpit **at rest** (the left armpit was
  clean — the asymmetry was the tell). The premise above was **wrong on two
  counts**: it *was* skinning, and it is *not* invisible at rest, because the
  skeletal-deformation visual params move the joints off the bindpose the base
  part's inverse-binds assume, so a wrongly bound vertex spikes wherever the
  rest deformation is non-trivial (the armpit). **Root cause:** the base-mesh
  joint-render-data list (`BevySkeleton::joint_render_data`, from **R2**)
  prepended each skin joint's **direct parent** as its ancestor; the reference
  viewer prepends the nearest **base-skeleton** ancestor
  (`getBaseSkeletonAncestor`, SL-287), *skipping* the extended (Bento) joints
  (`mSpine1`..`mSpine4`) that sit between `mTorso`/`mChest` and `mPelvis`.
  Including `mSpine2`/`mSpine4` inserted two extra render-list slots, shifting
  every later weight index by two — so a right-armpit vertex authored for
  `mChest` (raw weight `10.1`) resolved to `mElbowLeft` and was dragged across
  the body, and the whole left arm (weights `7`–`8`,
  `mShoulderLeft`/`mElbowLeft` in the reference list) bound to
  `mChest`/`mCollarLeft`. **Fix:** a
  `JointSupport` enum (`Base`/`Extended`) parsed from the `support` attribute in
  `sl-avatar`'s skeleton, carried into `BevySkeleton`, and a `base_ancestor`
  walk that skips non-base ancestors — the render list now matches the reference
  exactly and the skin displacements are symmetric. Confirmed live (own avatar,
  local OpenSim) top-down: the flap is gone. Because the whole arm chain was
  wrongly bound, this is expected to also fix — or substantially reduce —
  **R11** (the animation-time arm distortion), which should be re-checked next.
  New
  debug affordances added for this class of work (kept): `SL_VIEWER_CAMERA_*`
  (`ORBIT_DEG` / `ELEV_DEG` / `DISTANCE` / `TARGET_Z`) orbit the login-framing
  camera so the offline screenshot harness can capture a hidden spot, and
  `SL_VIEWER_LOG_AVATAR_GEOMETRY` logs per-part morph- and skin-displacement
  outliers (with each vertex's render-slot → joint name) — the tool that
  localised this. Surfaced by the R12 Firestorm side-by-side.
- [x] **R14. Base-body UV / clothing region mapping wrong at the extremities**
  (`sl-client-bevy` / `sl-bake`). Against a Firestorm side-by-side the baked
  clothing (the blue upper / red lower body layers) covered the **hands and
  feet**, which Firestorm leaves as bare skin, and there was a visible **gap /
  seam** in the coverage. **Localised** (offline screenshot vs the user's
  Firestorm shot): neither the base-mesh UVs nor the composite bounds — the
  fault was the **missing garment-shape masking**. A clothing layer's
  `local_texture` (the shirt / pants fabric) covers the *whole* body-region UV,
  including the hand and foot texels; the reference viewer bounds each garment
  layer to its garment extent by a stack of `avatar_lad.xml` `<param_alpha>`
  masks — sleeve length, shirt bottom, collar, pants length / waist, glove /
  sock / shoe / jacket bounds — driven by the wearable's shape params
  (`LLTexLayerParamAlpha` / `LLImageTGA::decodeAndProcess`). Our compositor
  blended each garment fabric across the whole region, so a solid-fabric
  shirt/pants painted the bare hands and feet. **Fix:** modelled the masks in
  `sl-bake` — a `ShapeMaskSpec` on each garment `PlannedLayer` (the static
  alpha-TGA, the driving param id, `multiply_blend`, `domain`), resolved by
  `region_layers` into compositor `ShapeMask`s (static TGA via the runtime's
  `static_image` closure + a new `mask_weight` closure); `composite_region` now
  multiplies each `LayerKind::Blend` texel's alpha by the combined mask
  coverage, reproducing the reference's per-`param_alpha` LUT (domain ramp /
  hard threshold) and additive-then-multiplicative accumulation
  (`renderMorphMasks`). The runtime preloads the mask TGAs (`shape_mask_files`)
  alongside the existing static layers. **The shape params are *driven*, not
  stored:** a garment stores only its group-0 driver (Sleeve Length 800, Pants
  Length 815, …), which drives the group-1 mask params (600 / 615 / …), so
  `mask_weight` runs the wearable's stored params through
  `ResolvedParams::from_values` (P13.4's driver→driven propagation, fed by a new
  `AppearanceValues::from_weights`) and reads the resolved driven weight — using
  the raw stored value instead left the sleeves/legs at the wrong length.
  Confirmed live (own avatar, local OpenSim, offline screenshot): hands and feet
  are now bare skin, the shirt sleeves are bounded, the pants end at the ankles,
  and the upper/lower waist seam is clean — matching the Firestorm ground truth.
  Surfaced by the R13 Firestorm side-by-side.
- [x] **R15. Terrain texturing wrong on Aditi** (`sl-proto` / `sl-client-bevy`).
  Root cause found (new `terrain-composition` conformance case, live on both
  grids): a modern Second Life mainland region leaves its four
  `TerrainDetail` ids **nil** in the `RegionHandshake` and drives the ground
  appearance another way, so the splat had nothing to fetch and rendered
  flat. This is *not* a parse bug — the case confirmed the `RegionInfo`
  fields that sit after the terrain block (`RegionID` / `ProductName` /
  `ProductSKU`) and the elevation bands all parse correctly while the ids
  are nil (aditi region "Mauve": `product_name = "Mainland / Full Region"`,
  `start_height 20` / `range 60`, all four detail ids nil). The reference
  viewer keeps rendering here because `LLVLComposition::setDetailAssetID`
  early-returns on a nil id, leaving the four
  **default Linden terrain textures** (dirt / grass / mountain / rock) its
  composition was seeded with. Fix: a new
  `RegionTerrainComposition::detail_textures_or_default()` substitutes those
  defaults (`DEFAULT_TERRAIN_DETAIL_TEXTURES`, in `sl-proto`) for nil slots,
  and the viewer requests the effective ids — the case shows all four
  defaults fetch and decode over `GetTexture` on aditi
  (`terrain_mode = "default-substituted"`, complete). A **second** bug
  (found by a live viewer run against aditi) stacked on top: the terrain
  composition is learned during the `RegionHandshake`, *before* the seed
  capabilities arrive, so the boosted `GetTexture` fetch failed permanently
  ("capability not available") and the ground stayed flat even with the
  defaults. Fix: the texture / mesh / wearable / animation managers now
  **hold** a request whose capability is not set yet and re-issue it once
  the cap arrives (`retry_pending*`), rather than fail it — a general
  latent-race guard (terrain is the only consumer that requests before caps,
  so it was the only one that reliably triggered it). Verified end to end by
  a windowed run: the aditi mainland ground renders the default dirt / grass
  / mountain / rock splat, matching Firestorm. Still deferred to
  **Phase 27**: a region that sends *non-nil* GLTF **material** ids (PBR
  terrain) — those do not decode as J2C, so the case marks that partial.
  Candidate cause (1), fetch-queue starvation, was already addressed by the
  Phase 20 `BOOST_TERRAIN` priority. Reference:
  `LLVLComposition::setDetailAssetID` / `getDefaultTextures`,
  `indra_constants.h` `TERRAIN_*_DETAIL`.
- [x] **R16. Linden system hair shows on mesh-hair avatars**
  (`sl-texture`). Surfaced during the P20.2 aditi session: the default Linden
  **system hair** base-mesh part (`avatar_hair.llm`, the helmet-shaped scalp
  mesh) kept rendering as a solid **dome** even on avatars that wear a **rigged
  mesh hair** attachment (or are bald), where the reference viewer hides it.
  **Root cause** (the third candidate — the hair bake's own alpha not being
  applied): a Second Life server "Sunshine" bake is a **5-component** J2C, whose
  channels are `R G B alpha mask` (the reference's `RGBHM`: colour,
  heightfield/**alpha**, clothing mask — `llviewertexlayer.cpp`). Our
  `decode_multicomponent` took only RGB and reported `components: 3`, so **every
  modern-SL bake was classified fully opaque** and the composited alpha (which
  makes a hair bake soft and a bald/mesh-hair bake transparent) was thrown
  away — the scalp mesh then read as a solid helmet and the P14.3
  transparent-region hide never fired. **Fix:** `decode_multicomponent` now
  keeps the first four
  channels — RGB **plus the composited alpha (channel 3)** — as the RGBA8 pixels
  (matching the reference viewer's `decodeChannels(.., 0, 4)`), so the existing
  P14.3 pipeline classifies a bald/mesh-hair hair bake `Transparent` (region
  hidden) or `Masked` (soft hair) with no rendering-code change. The 5th channel
  (the clothing/bump mask) is preserved in a new `DecodedImage::aux` field,
  mirroring the reference's separate `decodeChannels(.., 4, 4)` pass, for later
  material use; `downsample` carries it in lockstep. Confirmed live on aditi
  (own + nearby avatars): the hair dome is gone. Reference:
  `LLViewerTexLayerSetBuffer::readBackAndUpload` (`baked_image_components = 5`),
  `LLImageJ2C::decodeChannels`, `LLVOAvatar::updateMeshVisibility`.
- [x] **R17. Shoe height / heel offset not applied to avatar placement**
  (`sl-client-bevy-viewer`). Surfaced during the P20.2 aditi session: the worn
  **shoe** wearable's height adjustment — the heel / platform offset that raises
  the avatar so its feet rest on the ground — was not taken into account, so a
  shoe-wearing avatar sank into or floated above the ground. The body was
  planted only by the fixed pelvis rest height (P13.2), ignoring the shoe. The
  shoe height is **already a skeletal deformation** we resolve: the `Shoe_Heels`
  (id 197, driven by the transmitted `Heel Height` id 198) and `Shoe_Platform`
  (id 502) `param_skeleton`s offset `mFootLeft` / `mFootRight` downward in Z, so
  the reference viewer's `computeBodySize` folds that offset into
  `mPelvisToFoot` (`- foot.z * ankle_scale.z`) and stands the avatar taller.
  **Fix:** a per-agent `pelvis_lift`, computed from the resolved deformations as
  `-offset(mFootLeft).z * (1 + scale(mAnkleLeft).z)` (clamped ≥ 0 — a shoe only
  ever raises), is added to the pelvis rest height when planting the body root;
  `apply_avatar_appearance` re-plants an already-spawned, possibly-stationary
  body the moment its shoe lift changes (a disjoint anchor query) rather than
  waiting for its next position update. Unit-tested
  (`shoe_offset_lifts_the_body`); not visually confirmed against a shod avatar
  this session (the default own avatar wears no shoes and no second avatar was
  in view). Reference: `LLAvatarAppearance::computeBodySize` `mPelvisToFoot`,
  `avatar_lad.xml` `Shoe_Heels` / `Shoe_Platform` `param_skeleton`.
- [ ] **R18. Cloud layer — horizon plume fixed, one-quadrant clustering still
  broken** (`sl-client-bevy` / `sl-client-bevy-viewer`, P22.4). Noticed while
  verifying P23.1 water. Two distinct defects, one fixed, one **still open**:
  - **(fixed) Vertical horizon plume.** The old port evaluated the cloud UV
    *per fragment* from the view direction over a **full sphere**; near the
    horizon that projection is degenerate (`base_uv ∝ (1−cos elev)`, quadratic),
    smearing the texture into a vertical plume. **Fix:** render clouds on a
    CPU replica of the reference `LLVOWLSky` dome — the `calcPhi` zenith cap
    (φ∈[0,π/8]) with the reference **baked** planar texcoords
    (`buildStripsBuffer`, `((-z0+1)/2,(-x0+1)/2)`), and the camera-height offset
    (`DOME_OFFSET × DOME_RADIUS` = `0.96×15000`) baked into the vertices so the
    shallow cap wraps down to fill the sky (the reference puts the camera high
    inside the dome). `clouds.wgsl` now samples the interpolated vertex UV.
  - **(still open) Clouds cluster into ~one quadrant** with the other three
    near empty — on BOTH grids, not faithful (Firestorm spreads them evenly,
    reaching the horizon). Verified every checkable element of the port matches
    `class1/deferred/cloudsV` (the φ∈[0,π/8] dome, `calcPhi`, baked UV,
    `cloud_scale=0.4199`, the `0.96×15000` offset, the repeat sampler,
    `drawDome`→`mStripsVerts`) — there is only one cloud shader (no `class2`/
    `class3` variant; `LLVOClouds` gone), so the code path is right. Yet the
    dome projection maps the whole visible sky onto a tiny ~0.14-radius disc of
    the cloud texture (≈0.66 tile), so only 1–2 features show →
    one-sided. This mismatch with Firestorm's even clouds is **unexplained by
    source archaeology** and needs a same-grid Firestorm pixel comparison /
    runtime debugging. NOTE: a **separate confound was ruled out** — the EEP
    environment was not ingested on aditi at all (see R19), so aditi ran on
    WindLight defaults; with R19 fixed aditi now loads its real EEP and still
    shows the one-quadrant clustering, confirming a projection defect, not a
    settings problem. Candidate next step: the altitude-plane projection (sample
    the cloud texture where the view ray meets the cloud-altitude plane), which
    tiles evenly to the horizon — a deviation from the literal baked-UV formula
    but matches Firestorm's result. The `SL_VIEWER_LOG_CLOUDS` env var logs the
    live cloud EEP params + resolved texture id for comparison.
- [x] **R19. EEP environment never ingested on aditi (one-shot, no retry)**
  (`sl-client-bevy-viewer` / `sl-client-bevy`, P22.1). **Fixed.**
  Surfaced while debugging R18: on aditi the entire sky / sun / moon / cloud /
  star / water stack silently ran on the **legacy WindLight defaults**
  (`SkySettings::legacy_windlight_default`), never the region's real EEP. Root
  cause was a cap-not-ready-yet **race** (the same class as the terrain fetch):
  `request_environment` fired a **single** `RequestEnvironment` on
  `RegionHandshakeComplete`, and the runtime **silently drops it** if the
  `ExtEnvironment` capability is not in the caps map yet — which on a slower /
  remote grid it usually is not at handshake time. Local OpenSim seeds caps fast
  enough that the one-shot always won, so this went unseen until aditi. **Fix:**
  `request_environment` now retries every 3 s (up to 12 attempts) until
  `ingest_environment` folds the reply in and clears a pending flag (or it gives
  up to the defaults); a `RegionHandshakeComplete` (login or border crossing)
  starts a fresh cycle. The runtime also warns when `RequestEnvironment`
  finds no `ExtEnvironment` cap. Verified: aditi now logs `environment ingested
  (Region)` and the cloud params flip to `region_specified=true` with the
  region's real values. This retroactively means **any P22/P23 behaviour
  "verified on OpenSim only" was running on defaults on aditi** and should be
  re-checked there now that the real EEP loads.
- [x] **R20. Directional shadows oscillate along one axis**
  (`sl-client-bevy-viewer`, P24.1). **Fixed.** Noticed while verifying P25.2
  local lights: with a static camera and a stationary light prim, the sun/moon
  cascaded shadows on the ground jittered back and forth a small amount along a
  single axis, frame to frame. **Root cause** (confirmed by logging the
  per-frame light direction — 3196 unique values across 3221 frames): the day
  cycle runs off the real-time clock (`day_position` reads `SystemTime::now()`),
  so the sun rotates a hair **every frame**. Bevy's cascaded shadow maps already
  texel-snap the cascade origin
  (`bevy_light::build_directional_light_cascades` floors `near_plane_center` to
  texel multiples), but that snap is done in **light space** — a per-frame
  rotating light rotates the snap grid itself, so a fixed receiver lands on a
  different texel each frame and the shadow shimmers / oscillates (the
  back-and-forth is the `floor()` flip-flopping at a texel boundary). **Fix:**
  `snap_shadow_direction` (sky.rs) quantises the **shadow-caster** direction to
  a texel-equivalent angular grid (round the unit-vector components to
  `1 / shadow_map_size` and re-normalise) before orienting the `SceneSun`
  `DirectionalLight`. The direction is then bit-identical across the frames
  whose true direction stays in one cell (verified: it now holds for ~10–36
  frames even at fast dawn, far longer midday), so Bevy's texel snapping keeps
  the shadow perfectly still; each step moves any cascade's shadow by ≤ ~1 texel
  (imperceptible). Only the shadow projection is snapped — the visible sun disc,
  sky, and light colour keep the continuous direction. Verified live on OpenSim.
  Independent of Phase 25.
- [x] **R21. Large flat dark-blue plane across the scene (water / water fog?)**
  (`sl-client-bevy-viewer`, P23.1). **Fixed.** Noticed while verifying P26.3
  grass on the local OpenSim: a near-horizontal, near-uniform **dark blue**
  plane cuts across the scene at the shoreline, much darker and flatter than a
  plausible water surface — it reads as a solid slab rather than a lit, rippled,
  semi-transparent surface. **Root cause** (localised by an A/B capture — the
  slab vanishes with the underwater fog forced off, so it is the fog, not the
  `WaterMaterial` surface): the underwater-fog post-process
  (`underwater_fog.wgsl`) fogged **every** fragment below the water height,
  including the region's underwater **seafloor / terrain seen from *above*
  water**, painting it into a flat dark slab that shows through the
  semi-transparent water surface. The
  reference fogs the deferred *opaque* geometry **before** the transparent water
  surface is composited, so from above the surface shader alone gives the look;
  our fullscreen pass runs after everything, so it over-fogged. The contrast was
  starkest over the **void past a region edge with no neighbour** (endless-ocean
  surface, no seafloor → unfogged/light) against the adjacent region water
  (fogged seafloor → dark). **Fix:** gate the fog to an **underwater** effect —
  when the eye is **above** the water surface the shader returns the scene
  untouched (the `water.wgsl` surface provides the from-above deep-water tint +
  fresnel); only a **submerged** eye fogs the scene below, with the reference's
  per-fragment waterline clip preserved. Verified live on OpenSim (own captures
  above water + user confirmation both above and below the surface): the dark
  slab is gone, region water and void ocean now read the same, and submerged fog
  is unchanged. An earlier candidate (a `SURFACE_SKIP` band excluding only the
  water-surface *plane*) was tried and discarded — it left the fogged seafloor
  slab. Two debug affordances landed with this: a
  `SL_VIEWER_DISABLE_UNDERWATER_FOG` env A/B knob, and the `--camera-position` /
  `--camera-look-at` / `--camera-spin` / `--camera-spin-axis` CLI options (an
  absolute fixed camera pose + auto-rotate for unattended screenshot captures of
  a specific viewpoint, such as a region edge — the reproduction path this fix
  needed).
- **R22. Avatars stay low-detail / blue spheres / mesh-body render defects**
  (`sl-client-bevy-viewer`, P10 placeholders / P13 base avatar / P17 mesh
  attachments / P21 pixel-area LOD). Umbrella item, split into the distinct
  issues found while investigating it. The **original premise was
  disproven**: it read as "avatar baked skin / worn-mesh textures load coarse
  and never sharpen," but a live decode census showed 236/237 boosted avatar
  textures decode at full resolution and bound rigged meshes are never in the
  pixel-area-managed set — so a well-loaded avatar's textures and geometry are
  already full / finest. The "coarse avatar" symptom was really the far-avatar
  routing bug (R22a). The distinct issues:
  - [x] **R22a. Far / late avatar frozen in a static T-pose with coarse
    textures** (`objects.rs` / `meshes.rs`). **Fixed.** A worn rigged mesh
    whose `attachment_point` had not arrived by the time its mesh decoded was
    misrouted to the *static* (un-skinned) build path — leaving it in bind
    pose (T-pose) — and, via `worn_base_priority` returning `IDLE`, onto the
    pixel-area-*managed* LOD path for both geometry and textures, where a
    skinned mesh is never re-ranked, so it froze at the coarse level its rez
    distance warranted (worse the farther it rezzed, never recovering on
    approach). The rigged bind (`apply_rigged_attachments`) already resolves
    the wearer by parent chain, not `attachment_point`, so the routing gate
    was the sole cause. Now *any* rigged mesh routes to the skinned + boosted
    path regardless of `attachment_point`; a new
    `MeshManager::upgrade_to_finest` lifts a mesh discovered rigged off the
    managed / coarse-block path; its textures are boosted by the existing
    rigged build. A truly non-worn rigged mesh (animesh) defers to Phase 29.
    Verified live: an animated rigged-mesh avatar renders posed, not T-posed.
  - [x] **R22b. Coarse "blue sphere" avatars never resolve on approach.**
    **Not a bug — closed.** Root cause found live on aditi: the parcel we were
    testing on had the About-Land option *"Avatars on this parcel can see and
    chat with avatars on other parcels"* **unchecked**, so the region
    deliberately withholds other-parcel avatars' object data — they appear on
    radar/minimap only (our coarse sphere) and never stream a full object. This
    is a Second Life privacy feature, not a client fault; Firestorm shows the
    same spheres on such a parcel. It matched the telemetry exactly: every
    unresolved sphere had `ever_full_object=false` for the whole session and
    only the avatar co-located with us (same parcel) rendered, and camming the
    fly-camera to within ~6 m of a sphere never streamed it (camera position is
    irrelevant when the sim withholds the data by policy). The investigation
    still yielded three genuine Firestorm-parity omissions that were fixed and
    kept (they do not affect this parcel-privacy case): reporting the interest
    camera in fixed-camera mode, advertising `AgentHeightWidth`/`AgentFOV`, and
    advertising an `AgentThrottle`. Diagnostics behind
    `SL_VIEWER_LOG_AVATAR_INTEREST` (coarse census + per-avatar distance name
    tags) remain for any future interest-list work.
  - [x] **R22c. Mesh-body "universal" BoM slots render as flat placeholder
    skin** (`avatars.rs`). **Fixed.** A modern mesh body maps its arms / legs
    to the universal baked slots (`leftarm` / `leftleg` / `aux*`), which the
    viewer did not fetch — so those bake-on-mesh faces fell through to the flat
    skin placeholder, a tone seam against the UPPER-slot torso. Now the viewer
    fetches the universal bakes (new slot → service-name entries,
    `UNIVERSAL_BAKE_SLOTS`) and drapes them on the universal-slot BoM faces
    (confirmed live: the universal face resolves to a real bake). A correctness
    fix — it does not on its own resolve the arm's other defects (R22d–R22f).
  - [x] **R22d. Mesh-body arm renders semi-transparent** — the background
    bled through the arm. **Fixed** by the R22h clamp→wrap sampler change
    (user-confirmed on a normal skin). The earlier reference-faithful
    face-alpha work (`textures.rs`/`objects.rs`/`legacy_materials.rs`: a face
    no longer auto-blends just because its texture carries alpha — a
    `TextureAlpha` policy renders a rigged face opaque and an ordinary face
    alpha-*masked*, and `legacy_alpha_override` honours all four
    `DiffuseAlphaMode`s) was necessary but not sufficient on its own; the
    residual bleed was the arm's upper-region bake clamping to a transparent
    texture edge, which the GL_REPEAT fix resolved.
  - [x] **R22e. Green gap / seam line across the mesh-body forearm.**
    **Fixed** by the R22h clamp→wrap sampler change (user-confirmed). The
    "seam" was the forearm's upper-region UVs (`v ∈ [1, 2]`) clamping to the
    bake edge instead of wrapping — not a mesh geometry seam after all.
  - [x] **R22f. Hand redder than the arm on a mesh body.** **Fixed** by the
    R22h clamp→wrap sampler change (user-confirmed). The hand/arm tone
    mismatch was the same upper-region clamp artifact, not a
    `BODY_COLOR`-placeholder slot mismatch.
  - [x] **R22g. Other avatars' system body z-fights through their mesh body**
    (`avatars.rs`). **Fixed** (user-confirmed against a Firestorm side-by-side).
    A non-BOM mesh-body wearer hides the system body with a worn system **alpha
    layer**, which bakes the head / upper / lower regions to the `IMG_INVISIBLE`
    sentinel. We only hid the system body via the BOM (`IMG_USE_BAKED`) or a
    fully-transparent-classified real bake, and `is_bake_visible` *filtered*
    `IMG_INVISIBLE` out — so those regions had no hide signal and the untextured
    system body rendered and z-fought the mesh body (blotchy pale patches; live
    case: the avatar "Aciasblades", whose head/upper/lower slots are all
    `IMG_INVISIBLE`, rendered clean in Firestorm but blotchy for us). Now
    `invisible_body_slots` records the `IMG_INVISIBLE` base regions per avatar
    and `apply_avatar_part_visibility` hides them, matching the reference
    viewer's `isTextureVisible`. No-op for BOM / normal-bake avatars.
  - [x] **R22h. Mesh-body upper region (torso + arms) renders a flat white
    smear instead of its bake — a clamp-vs-wrap texture-sampler bug.** Root
    cause: `to_bevy_image` built every texture with Bevy's default
    **ClampToEdge** sampler, but Second Life samples with **GL_REPEAT** (the
    reference viewer sets clamp only for the rare TE clamp flag). A face whose
    mesh UVs sit on an **integer UV tile** — the mesh-body upper submesh here
    has `v ∈ [1.02, 1.99]` for **all** 57 740 verts — then clamps to the
    texture's edge texel instead of wrapping to the tiled image, painting the
    whole region the edge colour (on the grid-skin: white edge lines, with the
    magenta `(0,0)` corner where `u→0` — the "magenta bits"). The lower submesh
    happened to sit in `[0,1]`, so it rendered correctly under clamp, which is
    why legs worked and the torso/arms did not; other avatars with the same
    upper-tile UVs showed it too. **Fixed** (`sl-client-bevy` `to_bevy_image`
    now sets a Repeat sampler on all axes, keeping linear filtering — this also
    fixes tiled prim / terrain textures that need wrap). Pending live
    confirmation.
    Diagnosis path (for the record, since the first three hypotheses were
    wrong): a grid-skin A/B (a UV grid worn as the head/upper/lower skin
    bodypaint so both viewers fetch the *identical* server bake) → then a
    per-`(agent, slot)` BoM-resolution **tally** proved every BoM face *does*
    resolve its bake (`9(upper) 1/1`, `8(head) 1/1`, `10(lower) 1/1`), killing
    the "bake not applied / not fetched / read-as-not-visible" theories → then
    an offline check of the on-disk caches showed upper and lower bakes are
    byte-identical (**expected**, same grid bodypaint — not a cache bug) and
    the cached body mesh (`a2a889c4`) decodes with `v ∈ [1, 2]`. Two permanent
    diagnostics were added: the `apply_bom_face_materials` resolution tally
    (gated by `SL_VIEWER_LOG_AVATAR_FACES`) and the `mesh_uv_bounds`
    integration test in `tests/uv_seams.rs`. This likely **subsumes
    R22d–R22f** (the arm is upper region): re-evaluate them after confirming.
- [ ] **R23. Avatar stands too low — feet sink into the ground.** Our viewer
  renders the avatar with its feet buried below the terrain surface; in
  Firestorm the same avatar's feet rest *on* the ground. The avatar root is
  placed at too low a Z by roughly the ankle-to-sole height, so the whole body
  is offset downward. Candidates: a missing hover-height / foot-to-root offset
  (the reference positions the avatar so the *soles* meet the ground, not the
  pelvis-derived root), or the base-mesh / collision-volume foot offset not
  applied. Cosmetic but consistently visible. **Open.**
- [x] **R24. Neighbour-region avatars get no coarse dot — child-circuit
  `CoarseLocationUpdate` was dropped.** **Fixed.** `Session::dispatch_child`
  folded a neighbour region's object stream in (via `try_dispatch_object`) but
  had no arm for its `CoarseLocationUpdate`, so that message fell through to the
  unhandled-message diagnostic — only the *root* region's coarse (minimap) list
  reached the viewer, so an avatar present only in a neighbour region was never
  even placed as a coarse "blue sphere". Now both the root and child dispatch
  build the event via a shared `coarse_location_event` helper that tags it with
  the source circuit's `region_handle` (a new field on
  `Event::CoarseLocationUpdate`), and the viewer offsets a neighbour region's
  dots by `region − origin` metres (the same convention terrain uses, via the
  now-shared `metres_to_f32`) so they land on the right neighbour terrain rather
  than overlapping the home region. The viewer reconciles coarse dots **per
  region** (tracking each dot's source region), so a neighbour's update never
  despawns another region's dots; and `DisableSimulator` emits an empty
  `CoarseLocationUpdate` for the retiring region so its dots are dropped rather
  than left stale. Surfaced while investigating R22b but *separate* from it
  (that was a parcel-privacy case, root-region avatars).
- [ ] **R25. Prims that should be transparent render opaque.** On aditi, some
  plain prims that are transparent in Firestorm render fully opaque in the
  viewer. Picked two on a live region — the **Mauve sign** and the **fence
  around King Kong** — both plain **box** prims (`asset=None`,
  `path_curve=16`/`profile_curve=1`), large and flat (`scale≈10×0.26×8` and
  `0.24×9.77×2.92`), so this is a **prim**-face transparency path, not a
  mesh/sculpt or an avatar bake. Candidate causes to
  check: (1) a face whose **texture-entry tint alpha** is < 1 (the reference
  viewer's per-face `blinn_phong_transparent`) is not driving the material's
  `AlphaMode::Blend` — the prim face path only alpha-*masks* off a texture's own
  alpha channel (R22d), and a genuinely translucent tint should blend; (2) a
  face carrying an `LLMaterial` / GLTF **diffuse alpha mode** of `BLEND` (the
  Phase-27 `legacy_alpha_override` / PBR material path) not being applied to a
  prim face; (3) a **fullbright + alpha** or "alpha mode: alpha blending"
  legacy-material face. Reproduce with the `P` pick tool on a known-glass prim
  and log its `TextureFace` colour alpha + any material override before deciding
  which path is dropping the transparency.

## Non-goals (deferred; candidate follow-up roadmaps)

Most former non-goals are now planned phases (see Phases 19–34): PBR / GLTF and
legacy normal / specular materials + bump / shiny / glow / fullbright
(Phase 27), animated textures (28), water surface (23), sky / atmosphere (22),
shadows (24), distance-based LOD switching (21), local lights (25), Linden
trees / grass (26), animesh (29), particles (30), flexi prims (32), reflection
probes (33), and avatar cloth / breast-butt physics (34), on a shared `avian3d`
physics foundation (31). Still deferred: facial-morph lip-sync, object
selection / interaction, any chat *input* or non-quit UI, and sound.
