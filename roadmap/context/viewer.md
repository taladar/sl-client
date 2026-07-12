# Context — VIEWER_ROADMAP.md

Non-task preamble carried over from `VIEWER_ROADMAP.md`. Tasks split out of that
file carry the `viewer` topic.

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
