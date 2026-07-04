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
- **`sl-client-bevy`** — a small addition: a `to_bevy_prim_mesh` conversion +
  re-exports, mirroring the existing `to_bevy_mesh` / `to_bevy_image`.
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

- [ ] **P8.1. Map → grid.** The crate takes a decoded RGBA8 sculpt map
  (`sl_texture::DecodedImage`) + `sculpt_type` / flags and returns
  `sl_prim::PrimMesh`. Resample to a fixed working size (bilinear); pixel
  `(r, g, b) / 255 - 0.5` → a grid vertex. The crate itself stays I/O-free
  (like `sl-prim`): it never fetches or decodes. The `DecodedImage` it consumes
  must be sourced from the shared `TextureStore` (the same fetch /
  off-thread-decode / disk-cache pipeline the Phase 6 texturing drives), which
  the viewer supplies at P9.1. Do not add an inline JPEG-2000 decode here.
- [ ] **P8.2. Stitch modes.** Stitch per type — plane (no wrap), cylinder
  (wrap U), sphere (wrap U + collapse the pole rows), torus (wrap U + V); honour
  the mirror / invert flags (winding / normals). Build indices, per-vertex
  normals, and grid UVs; emit a single `PrimFace`. Fall back to a placeholder
  grid on a degenerate map (never panic).
- [ ] **P8.3. Stitch tests.** Unit tests per stitch type (counts; seam and pole
  vertices are shared, not duplicated). `cargo test -p sl-sculpt`.

## Phase 9 — Sculpt rendering in the viewer

- [ ] **P9.1. Sculpt objects.** For `SculptOrMeshKey::Sculpt(texture_key)`,
  fetch + decode that sculpt map **through the same Phase 6 `TextureManager` /
  `TextureStore`** (request the texture id, react to its `TextureDecoded`, read
  the decoded `DecodedTexture` pixels as geometry input — reusing the store's
  fetch / off-thread-decode / disk-cache, not a fresh inline decode); the object
  stays in the "waiting on asset" state as a mesh does. Feed the pixels + type
  into `sl_sculpt`, convert with `to_bevy_prim_mesh`, and texture via Phase 6.

## Phase 10 — Avatar placeholders

- [ ] **P10.1. Spheres.** Track avatars from `ObjectAdded` (pcode 47) and
  `CoarseLocationUpdate`; render each as a ~2 m UV-sphere `StandardMaterial` at
  the (converted) position; despawn on removal or when dropped from the coarse
  locations. No rig, baked textures, or animation. Verify with a second
  logged-in avatar.

## Phase 11 — Chat overlay

- [ ] **P11.1. On-screen chat.** A `bevy_ui` `Text` node pinned to a corner; on
  `ChatReceived` append `"{from_name}: {message}"` (shout / whisper as a prefix
  label), keep the last N lines bottom-up. Read-only, no input box. Verify with
  chat from the second avatar.

## Phase 12 — Live verification & polish

- [ ] **P12.1. OpenSim end-to-end.** Window opens, login succeeds, and terrain +
  prims + textures + mesh + sculpt + avatar-spheres + chat all render; fly
  around; `Esc` logs out cleanly and exits with success.
- [ ] **P12.2. Aditi smoke (optional).** Run against `credentials.aditi.toml`
  with the MFA wrapper; expect terrain + server-baked-avatar spheres — prim /
  mesh content depends on the landing region.
- [ ] **P12.3. Clean sweep.** `cargo clippy --workspace --all-targets` clean,
  `cargo fmt --all`, `rumdl` on this file; tick the remaining boxes.

---

## Non-goals (deferred; candidate follow-up roadmaps)

Advanced materials (PBR / GLTF `GltfMaterialOverride`, legacy normal / specular
`RenderMaterials`, bump / shiny / glow / fullbright), avatar meshes / rigging /
baked textures / animation (spheres only), flexi / particles / lights /
reflection probes, water surface, sky / atmosphere, distance-based LOD switching
(fixed High LOD), object selection / interaction, any chat *input* or non-quit
UI, and sound.
