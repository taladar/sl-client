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

## Implementation notes & gotchas (cross-cutting, from build history)

- **Attachment-point type DUALITY (P16.1, matters for P16.2/P19):** there are
  TWO `AttachmentPoint` enums — `sl_types::attachment::AttachmentPoint`
  (`Avatar(..)|Hud(..)`, what `Object::attachment_point()` returns) and
  `sl_proto::AttachmentPoint` (flat, `Other(u8)`, what sl-avatar's
  `AttachmentPoints` table is keyed by). They are NOT interchangeable. The
  viewer's attach parenting sidesteps both by bridging on the **raw u8 point
  id**: `Object::attachment_point_id()` ↔ `AttachmentPointDef.id`. The
  `avatar_lad.xml` `<attachment_point>` table gives point-id → joint NAME; the
  skeleton resolves name → index. Of 55 points, 47 non-HUD resolve to a body
  joint, 8 HUD (`mScreen`) don't (Phase 19). `mRoot` (avatar-centre, id 40) is
  **not** in `avatar_skeleton.xml` — LL synthesizes it in code, so the viewer
  appends a synthetic identity `mRoot` root above `mPelvis`
  (`BevySkeleton::insert_synthetic_root`, appended at END so joint indices stay
  stable; body skeleton is then 134 joints). Attachment parenting is a separate
  `adopt_pending_attachments` system (not literally `reconcile_parent`) because
  the avatar's joints live in `AvatarState`/`AvatarBody`, resources
  `update_objects` can't reach.
- **P16.2 attachment transform + how to LIVE-verify a worn attachment (reused
  for P17 rigged-mesh):** the reference viewer inserts an
  **attachment-point node** between joint and object (`LLViewerJointAttachment`,
  at the `avatar_lad.xml` point `position`/`rotation` offset), and the worn
  object's own local transform is relative to *that node* — so P16.1's direct
  joint-parenting was one offset short. Fix spawns one node entity per point per
  avatar (`AvatarState::attachment_nodes`); the offset stays in the joint's SL
  Z-up frame like a linkset child, and `coords::sl_euler_deg_to_quat` copies
  `LLQuaternion::setQuat(roll,pitch,yaw)` verbatim. `ADD_FLAG` is a no-op for
  the renderer (bit already stripped in sl-proto; replace = server
  `KillObject`). The headless viewer renders BOTH the own avatar and others'
  avatars as rigged bodies, so the reusable live check for anything worn: run
  **two** avatars in the same region (both
  `--start "uri:Default Region&128&128&30"` to be ROOT on the 2×2 megaregion) —
  one wears via `sl-repl-tokio --script` (`rez_attachment <item> chest`;
  leftover attachable "Object" cubes sit in the Objects folder
  `5803edc5-297b-4bcd-9dba-e52be202f4a4` from attach-detach conformance runs),
  the other observes via the viewer and logs
  `parented attachment … (point N) to avatar … joint`. `grim` grabs the whole
  desktop, not the viewer window, so it's poor for framing the avatar — rely on
  the log line.
- **P17.1 rigged-mesh skinning math (`sl-avatar` `skin.rs`, pure) DONE — key
  handoff for P17.2:**
  `SkinningPalette::build(&MeshSkin, |joint_name| Option<[f32;16]>)` +
  `skin_position`/`skin_normal`. Deliberately **glam-free / Bevy-free** —
  hand-rolled `[f32;16]` row-vector row-major mat-mul + affine transform (SL's
  convention, same layout `sl-mesh` decodes), NOT the glam `Mat4` the P13.1 bevy
  layer uses. So P17.2's bevy consumer must BRIDGE: it poses the skeleton
  instance and supplies each rig joint's **current world transform** as
  `[f32;16]` (that's where `alt_inverse_bind` / `pelvis_offset` /
  `lock_scale_if_joint_position` get honoured — they shape the world matrices,
  the palette algebra ignores them). Missing-joint → world=identity → palette
  entry is the bare inverse-bind (matches Firestorm
  `initSkinningMatrixPalette`). Weights renormalized per-vertex. New
  `sl-avatar → sl-mesh` dep. NOTE: this is the RIGGED-mesh path
  (`MeshSkin`/`VertexWeights`); the legacy base body's 2-joint
  `VertexSkinWeight` blend is a separate path (bevy `avatars.rs`), so the
  README's "shared by base body and rigged mesh" wording is aspirational.
- **P17.2 rigged-mesh rendering DONE (live-verified on aditi) — key facts NOT in
  roadmap/git:** (1) it uses **Bevy GPU `SkinnedMesh`** (same path as the base
  body), **NOT** the P17.1 CPU `SkinningPalette` — so P17.1's pure helper stays
  UNUSED by the viewer (a parallel pure-crate impl; confirms the "shared"
  wording is aspirational). The bevy bridge (`to_bevy_rigged_mesh` +
  `rigged_inverse_bindposes` in `sl-client-bevy`) builds
  `JOINT_INDEX`/`JOINT_WEIGHT` and folds the bind-shape into each inverse
  bindpose; matrix layout — `Mat4::from_cols_array(& row_major[f32;16])` IS
  exactly the row→column-vector transpose Bevy skinning needs, so no explicit
  transpose. (2) **THE bug that ate the session:** mesh bodies/clothing rig to
  the avatar's **collision volumes** (`PELVIS`, `BELLY`, `L_UPPER_ARM`, … — the
  uppercase `<collision_volume>` names, distinct from the `mPelvis` bones),
  which `BevySkeleton` did NOT expose as joints (they live in
  `Joint::collision_volumes`, out of the joint list/lookup). Every CV weight
  fell back to the pelvis → clothing ballooned into a **sphere** spanning the
  avatar. Fix: `BevySkeleton::from_skeleton` appends each bone's CVs as extra
  joints (parented to the bone at the XML pos/rot/**scale** — ref viewer
  `setupBone` `setScale`s a CV like a bone, and that scaled world matrix is what
  the mesh inverse-binds cancel). (3)
  **REUSABLE live-verify recipe for P17.3/P18** (OpenSim CANNOT — its avatars
  are system-body, no worn rigged mesh): run the RELEASE viewer against
  **aditi** with `--viewer-assets <firestorm>/indra/newview/character/` and
  `RUST_LOG=info,sl_client_bevy_viewer::objects=debug`; watch for
  `bound rigged mesh <uuid> on avatar … to its skeleton` and (the diagnostic
  that cracked the bug) the `N/M joint(s) unresolved, bound to pelvis` warn. A
  real aditi mesh-body avatar binds ~6 rigged meshes. (4)
  **The mesh BODY itself renders invisible** because its skin faces use
  `IMG_USE_BAKED_*` BoM UUIDs (fetched as normal textures → 404) — this is the
  **P17.3** deliverable, not a P17.2 bug; user asked to note in the roadmap that
  P17.3's BoM placeholder must be OPAQUE (skin tint), not transparent, so a
  not-yet-fetched body isn't a bodiless shell.
- **Runtime read-model exposure DIVERGES by runtime** (set in CHAT A10; relaxes
  PERMISSION's "all reads via Event"): sans-IO `Session` keeps zero-copy read
  accessors; **bevy** systems read by direct `&Session` borrow; **tokio + REPL**
  use a pull bridge — a query `Command` → a synthesized reply `Event` carrying
  `Arc<[…]>` snapshots / paged cursors (the
  `QueryScriptPermissions`→`ScriptPermissionState`, chat-history, and inventory
  pull-bridges all follow it). Parity = identical data + commands + view types;
  only the read *transport* differs.
- **sl-types Ord-derive exception:** CHAT B1 added `PartialOrd, Ord` to
  `Key`/`FriendKey` (sl-types 0.6.1 also covers `AgentKey`/`GroupKey`/
  `ImSessionId`) for the BTreeMap/BTreeSet keys — crosses the normally
  consume-only sl-types policy; additive, accepted.
- **Workflow gotchas that recur** (beyond the workspace clippy conventions): the
  ggh pre-commit runs `cargo nextest`, which runs all crates' test binaries in
  parallel → byte-identical tokio+bevy shell tests sharing a
  `std::env::temp_dir()` path race/flake; namespace per-test temp dirs by
  `env!("CARGO_PKG_NAME")`. `cargo doc -D rustdoc::private_intra_doc_links`
  forbids a `pub` item's doc linking a private item (use a plain code span).
  Runtime fs must use `fs_err::*` (clippy.toml disallowed-methods).
- **VIEWER "R" known-issues pass is an interactive aditi live review** (fix →
  screenshot/pick → fix, NOT autonomous — empty OpenSim can't reproduce most of
  it). Live-debug tooling now in the viewer (details of the *fixes* are in the
  `viewer` roadmap topic R4–R7 + git; these are the reusable *enablers*): env
  `SL_VIEWER_LOG_OBJECTS=1` flags region-sized / sky objects
  (`log_suspicious_objects`); pressing **`P`** is a crosshair pick
  (`pick_object`) that raycasts from the camera and logs the object under
  screen-centre — full_id, mesh/sculpt `asset`, `scale`, **`world_scale`** (the
  object entity's actual GlobalTransform scale; `world_scale ≫ scale` is what
  proved the linkset root-scale-propagation bug), and the raw `PrimShapeParams`;
  and `sl-client-tokio`'s `rez_sample_prims` example populates an empty grid
  with one prim of each volume type (box / cylinder / prism / sphere / torus /
  tube / ring) + tiled/rotated/offset texture demos. KEY debugging enabler: the
  viewer **disk-caches every asset it renders** — meshes at
  `~/.cache/sl-client-bevy-viewer/meshcache/<uuid>.mesh` (raw LLMesh bytes),
  textures + sculpt maps at `…/texturecache/` — so a misrendered asset is
  decoded OFFLINE exactly as the viewer saw it, no re-login. Heuristic that
  scoped the bugs: prim/mesh/sculpt geometry is ALL normalized to ~unit-cube
  then ×`object.scale`, so an oversized render is scale-propagation (Bevy
  composes a parent's scale down the linkset; SL prims are absolute-size → fixed
  by moving scale onto a per-object geometry-holder child) or shape
  tessellation, never the fetch.

Then **Phase 18 (animations) STARTED — P18.1 scaffold `sl-anim` + `.anim`
decode DONE** (pure crate, `cargo test -p sl-anim`, clippy/doc/rumdl-clean; full
design in the roadmap P18.1 Done note). The one durable directive that fell out
and is NOT in git/roadmap: on my asking whether to skip the legacy `0.1` `.anim`
form, the user said **support legacy asset formats too** — 20+-year-old SL
content (esp. animations) is never replaced by visual updates, so decode every
version branch the reference viewer handles, not just the modern one
((support legacy asset formats)); `sl-anim` decodes both `1.0`
(`u16`-quantised) and `0.1` (`f32` Euler via a `mayaQ`/`ZYX` port).

**P18.2 (resolve `anim_id`→asset + cache) DONE** (roadmap P18.2 Done note has
the full design). Facts NOT in git/roadmap:

- **The procedural-vs-keyframe split is THE gotcha for P18.3/P18.4.** Of the 140
  built-in `ANIM_AGENT_*` UUIDs, **48 are procedural** — walk/run/stand/turn,
  the `LLEmote` facial expressions, and the always-on adjusters
  (head/eye/hand/breathe/physics). The reference viewer synthesises these in C++
  (`llvoavatar.cpp` `registerMotion` with a class ≠ `LLKeyframeMotion`) — there
  is **no downloadable `.anim` asset**, so fetching their UUID over the asset
  cap 404s. The resolver records them unavailable and never fetches; P18.3/P18.4
  must either synthesise or skip them. The other 92 (waves/bows/dances) ARE
  ordinary `.anim` assets on the asset server under the fixed UUID, fetched
  exactly like uploads. Data ported from `llcharacter/llanimationstates.cpp`
  (UUIDs) + `registerMotion` (which are procedural).
- **Registry lives in `sl_anim::registry`** (`BuiltinAnimation`/`BuiltinKind`/
  `builtin_animation(uuid)`), module **role-named to dodge
  `module_name_repetitions`** like `decode`. Named `registry` NOT `builtin`
  (repeats the type names) and NOT `library` (user: clashes with `lib.rs`).
- **The viewer depends on `sl-anim` DIRECTLY** (like sl-terrain/sl-texture), not
  via a sl-client-bevy re-export — `sl-anim` is a pure crate with no
  store/fetcher, so like **sl-prim it's a parity exception** (no sl-client-tokio
  counterpart / no animation driver there yet). The fetch reuses the existing
  `AssetStore`/`BevyAssetFetcher`/`ViewerAsset` infra from the P15.2 wearable
  path (`AssetType::Animation`); the new viewer `animations.rs`
  `AnimationManager` mirrors `WearableAssetManager`/`MeshManager`.
- **Live-verify limitation (reused for P18.3):** a passive OpenSim login only
  ever sees its OWN `stand` in `AvatarAnimation` — a **procedural** built-in —
  so the download+decode branch (and ANY visible keyframe motion) can't be
  exercised by an idle login. To exercise a downloadable animation you need an
  avatar actually PLAYING one: `sit`/`sit_ground` (both Keyframe), or a 2nd
  avatar's gesture via the P16/P17 two-avatar recipe. The viewer has no
  avatar-control input. Verified live on OpenSim only that `stand` is ingested →
  registry- resolved → classified procedural → not fetched.
- **The `--viewer-assets` local `<uuid>.anim` branch is near-theoretical** —
  stock Firestorm ships NO built-in `.anim` files (its `character/` dir is
  xml+llm+tga only), so downloadable built-ins arrive over `ViewerAsset` like
  uploads; the local-file lookup is an escape hatch for a hand-populated
  library.
Then **P18.3 (drive the skeleton) DONE** (roadmap P18.3 Done note + R11 have the
design/history; `AnimationDecoded` was removed — the driver polls the motion
cache each frame instead). Non-obvious facts NOT in git/roadmap, mostly about
how to LIVE-TEST animations:
- **OpenSim ships the standard SL animation assets** under
  `~/devel/3rdparty/opensim/bin/assets/AnimationsAssetSet/` (`<uuid>.dat` +
  `index.xml`), and they ARE fetchable over `ViewerAsset` — so a built-in like
  `dance1` (`b68a3d7c-…`) or `clap` (`9b0c1c4e-…`) downloads + decodes on the
  LOCAL grid, no aditi needed. This is the reliable way to exercise the
  download/ decode/drive path (P18.2 could only ever see the procedural
  `stand`).
- **Test animations on the OWN avatar via `--play-animation <uuid>`** (single
  login: the viewer sends `Command::PlayAnimation` on handshake and the sim
  echoes the agent's own `AvatarAnimation` back). The two-avatar recipe kept
  FAILING here because re-logging the SAME avatar (e.g. secondary) after a
  `kill`/timeout'd session is blocked by OpenSim's stale-presence cleanup for
  ~60-90 s, so the dancer entered the region only AFTER the viewer's window
  closed. Own-avatar play sidesteps it. The camera already frames the agent
  head-on on login (session.rs).
- **`grim`/screenshot-during-run is unusable**: approving the screenshot Bash
  tool moves window focus off the viewer, so it never captures the viewer
  window. Rely on the user's own screenshots + log-based diagnostics (e.g. a
  temporary per-frame bone-length dump proved the SKELETON is posed correctly
  under animation — bones stay constant — which is how R11 was pinned to the
  base-body SKIN, not the driver).
- **`dance1` is a one-shot** (`loops=0`, ~3 s); many SL "dance"/"clap" built-ins
  are one-shot (the AO re-triggers them). A LOOPING keyframe built-in on OpenSim
  is `hover` (`4ae8016b-…`, 7.5 s) if you need a sustained view. `.anim`
  position keys (chiefly `mPelvis`) are RELATIVE offsets (start at 0, grow) —
  add to rest, never replace (replacing collapses the pelvis ~1 m).

Then the **R-section rendering fixups** (the roadmap/ viewer topic, `R1`–`R14`;
read the roadmap for each). Cross-cutting/reusable facts from the
R11/R12/R13/R14 session NOT in git/roadmap:

- **R14 gave `sl-bake` a garment-shape `param_alpha` masking engine — reuse it
  for any future clothing work.** What keeps SL clothing off the bare hands/feet
  is NOT the base-mesh UVs, a static region mask, or the composite bounds — it
  is each garment layer's stack of `avatar_lad.xml` `<param_alpha>` masks
  (`LLTexLayerParamAlpha` → `LLImageTGA::decodeAndProcess` LUT: a `domain` ramp,
  or a hard threshold at `1 - weight`, `multiply_blend` = min / additive = max,
  seeded per `renderMorphMasks`), driven by the *wearable's own* shape params
  (sleeve length 600, pants length 615, waist, collar, glove/sock/shoe/jacket
  bounds). A garment's `local_texture` covers the whole region UV, so without
  the masks a solid-fabric shirt/pants paints the whole limb. Now modelled as
  `ShapeMaskSpec` on each garment `PlannedLayer` + a `mask_weight` closure on
  `region_layers` + `shape_mask_files()` preloaded beside
  `static_layer_files()`; `composite_region` multiplies each `Blend` texel's
  alpha by the mask coverage.
  **KEY GOTCHA: the shape params are DRIVEN, not stored.** A garment stores only
  its group-0 driver (Sleeve Length 800, Pants Length 815, Shirt Bottom 801,
  Waist 814), which drives the group-1 mask params (600/615/601/614) — so
  reading `asset.params.get(&600)` gets nothing and the raw fallback puts the
  garment at the wrong length (sleeves/legs too long). The weight must come from
  running the wearable's stored params through P13.4's driver→driven resolver
  (`ResolvedParams::from_values`, fed by the new
  `AppearanceValues::from_weights` I added to build resolver input from an
  id→weight map instead of a wire vector). With that, pants end exactly at the
  ankle matching Firestorm. Only the garment-shape alphas were added — the
  procedural cosmetic/bump layers (shading, make-up, freckles) are still
  deliberately out of the compositor's scope.

Earlier facts from the R11/R12/R13 half of the session:

- **A Firestorm↔local-OpenSim side-by-side is now THE reference for base-body
  rendering** (set up this session; run recipe + comparison steps are in the
  sl-client skill). There was never a reference viewer for the system avatar
  before, so "verified live" P13/P15 items shipped latent shape bugs — R12 (own
  avatar bloated at rest) only surfaced once Firestorm rendered the same account
  correctly. Verify any appearance / morph / skinning change this way. Gotcha:
  add the grid in Firestorm's OpenSim prefs as `http://127.0.0.1:9000/` — the
  IPv4 literal, NOT `localhost` (resolves to `::1`; OpenSim listens IPv4-only,
  so a `localhost` grid fails login with a spurious `Http_503`). OpenSim's
  `get_grid_info` `[GridInfoService] login` was corrected to `:9000` (it
  defaulted to the unbound ROBUST `:8002`).
- **The P18.3 "grim screenshot unusable" note is SUPERSEDED**: the viewer now
  has `--screenshot-dir` (saves a numbered PNG sequence after a tunable startup
  delay — `SL_VIEWER_SCREENSHOT_{DELAY,INTERVAL,FRAMES}` — then quits; cursor
  left un-grabbed so it doesn't hijack the desktop) + `--repeat-animation`
  (Stop+Play re-issue every ~2 s so a one-shot `dance1` still plays once the
  avatar has loaded). Read the PNG files back with the Read tool; crop at full
  res with PIL. This is how R11/R12 were inspected without a live operator at
  the window.
- **R12 architecture lesson — the OWN avatar's shape is rendered from the
  SERVER-echoed `AvatarAppearance.visual_params`** (`apply_avatar_appearance`
  caches them in `AvatarState.appearances`), NOT the worn wearable directly. So
  a placeholder appearance PUBLISH self-corrupts per account: `bake_publish`
  sent an all-`128` vector, the sim stored + rebroadcast it, and we rendered it
  — a never-corrected account stayed bloated forever (a Firestorm login,
  publishing the real shape, permanently fixed it). `128` is the range MIDPOINT,
  wrong for asymmetric body morphs (default `0`) → every one half-applied →
  bloat + head spikes. Fix resolves the real params from the worn wearables
  (`OwnBakeInputs::visual_params` + new `VisualParams::encode_appearance` /
  `f32_to_u8`, the inverse of `map_appearance`) for BOTH publish and render
  (`apply_own_shape_from_wearables` overrides the echo, self-heals a re-outfit).
- **R11's skin-pivot premise was a proven sub-mm no-op** — every base-body `m*`
  joint has identity rest rotation, unit rest scale and `pivot ≈ pos` (<1 mm
  accumulated on every animatable chain), so the reference `translate(-Σpivot)`
  skin offset equals our `bind_globals⁻¹`; porting it changes nothing. The real
  animation distortion was a wrong per-vertex joint mapping — invisible at rest
  where every skin matrix is identity. **R11 and R13 were ultimately the SAME
  bug**, at two visibilities: the base-mesh joint-render-data list included the
  extended (Bento) `mSpine*` ancestors the reference viewer skips, shifting
  every weight index past them (R13's `base_ancestor` fix). R13 was the
  rest-visible face of it (an armpit spike where the shape deformation is
  non-trivial); R11 was the animation-time face (whole arm chains bound to the
  wrong joint, so gross distortion the moment a joint rotated). Fixing R13 fixed
  R11 with **no additional code** — R11 was closed as a pure re-check (verified
  live: shaped own avatar dancing `dance1`, head-on + a 50° orbit, limbs skin
  cleanly across the full pose range). So a "skinning looks fine at rest but
  wrong under animation" base-mesh symptom → suspect the render-list
  weight-index mapping, not the skinning math.

Then **P18.4 (priority blending) DONE** (roadmap P18.4 Done note has the
design/history). Cross-cutting facts NOT in the roadmap:

- **Reference sources for the blend/ease, if it ever needs revisiting:** the
  per-joint blend is `LLJointStateBlender::blendJointStates` in
  `indra/llcharacter/llpose.cpp` (highest-priority-first fill of a weight
  budget, 4 slots = `JSB_NUM_JOINT_STATES`); the ease weight is the
  `setWeight(...)` ladder in `LLMotionController::updateMotionsByType`
  (`indra/llcharacter/llmotioncontroller.cpp`), and `cubic_step` (smoothstep) is
  in `indra/llmath/llmath.h`. A `.anim` keyframe motion is always `NORMAL_BLEND`
  (never additive), so the additive-blend path is intentionally NOT modelled.
- **Equal-priority ordering quirk (reproduced on purpose):** the reference keeps
  `mSignaledAnimations` as a `std::map<LLUUID,S32>` (UUID-sorted) and pushes
  each newly-`startMotion`'d motion to the FRONT of `mActiveMotions`. So an
  observer present as each animation starts sees the **last-started** one win a
  tied joint, but an observer arriving later starts them all at once from the
  sorted set, so the **highest-UUID** one wins instead — a genuine SL bug we
  mirror. Our driver reproduces both from one rule: a per-avatar monotonic
  activation-order stamp assigned to newly-activated anims **in UUID order**
  within each update (`reconcile_playing`), used as the tie-break in
  `blend_joint`.
- A sequence-id change for the same anim UUID counts as a re-activation (fresh
  stamp, clock reset) — matches the reference's "different sequence id → restart
  motion" in `LLVOAvatar::processAnimationStateChanges`.
- **Testing affordance:** `--play-animation` is now **repeatable** / comma-sep
  so a single own-avatar login can layer several anims. Live blend check on
  OpenSim used `dance1` (`b68a3d7c-…`) + `clap` (`9b0c1c4e-…`).

After P18.4 the roadmap was RE-PLANNED (commit 36ad10c) — Phases 19–33 are now
rendering-fidelity work + R15 aditi terrain, not the old "HUD attachments /
aditi verify". HUD attachments moved to Phase 34, aditi real-SL to Phase 35.

Then **P19.1 (FPS + frame-time overlay) DONE** — first point of new Phase 19
(Diagnostics HUD). New viewer module `diagnostics.rs`; the viewer adds Bevy's
`FrameTimeDiagnosticsPlugin` + `EntityCountDiagnosticsPlugin` and a persistent
top-right, right-justified `Text` node rewritten each frame. Non-obvious fact
NOT in the roadmap: Bevy 0.19 exposes **no draw-call diagnostic** (the only
render-side one, `RenderDiagnosticsPlugin`, is GPU pass timing), so the "draws"
figure is the live `Mesh3d` instance count via a `Query<(), With<Mesh3d>>` — a
coarse per-frame draw gauge. The frame/entity numbers come from the smoothed
`FPS` / `FRAME_TIME` / `ENTITY_COUNT` diagnostics (`Diagnostic::smoothed`).
`SlClientPlugin { diagnostics: true }` is unrelated (protocol diagnostics), so
no double-registration.

P19.2 (pipeline status API) DONE — see the roadmap "Done" note for the shape.
Two facts worth carrying forward: (1) `stats()` treats each entry's per-entry
progress enum as the source of truth for its bucket, which is what promoted a
latent bug to load-bearing — the texture/mesh `get()`/`set_lod()` direct paths
never published a terminal `Ready`/`Failed` (only `request`/`drive` did), so a
`get()`-loaded entry read as stuck `Decoding`; fixed via a shared `publish()`
helper. `AssetStore.get()` already published terminal progress, so it was clean.
(2) P19.2 is library-only (no overlay until P19.3) and the progress fix is inert
for rendering (only the observable field changed, not fetch/decode), so there
was nothing to verify live this phase. Next open VIEWER item is P19.3 (key-
toggled HUD panel rendering the P19.2 texture+mesh counts).

Then P19.3 (pipeline status overlay) DONE — see the roadmap "Done" note for the
shape. Non-obvious live-observability caveat worth carrying forward for the
Phase 20/21 priority+LOD work that will watch this panel: on the running viewer
the per-stage buckets (`queued`/`dl`/`dec`/`ready`) and `in_memory`/`bytes` read
**0 even after the region's textures are clearly loaded and drawn**, because
`stats()` only counts entries the weak-ref store still holds a live
(upgradeable) ref to — and once a texture is decoded and handed to the viewer's
own `decoded` HashMap / Bevy image, the store keeps no strong ref, so the entry
is not "in memory" from the store's point of view. The proof the pipeline ran is
the **cumulative** `cached` (disk-cache-hit) / `gc` counters, not the
instantaneous buckets. So for a live health read, watch
`cached`/`queued`/`dl`/`dec` churn, not `ready`/`mem` (which idle back to 0).
Verified live on OpenSim at a steady state: `tex cached 14 gate 0/16 wait 0`,
everything else 0.

**Phase 19 (Diagnostics HUD) DONE.** Then
**P20.1 (screen-importance / pixel-area helper) DONE** — see the roadmap "Done"
note for the shape. Placement call not in git: put it in `sl-asset-sched` (new
`screen` module, `ScreenMetrics`) rather than a new crate — Phase 20 lists no
new artifact, and it sits next to the `Priority` P20.2 maps it onto in the same
domain-free crate. Pure helper, so verified with `cargo test -p sl-asset-sched`
only (no live viewer run this point). Next open VIEWER item is P20.2 (drive
fetch priority: map pixel area → `sl_asset_sched::Priority`, feed
`TextureStore`/`MeshStore` `request` + re-prioritize per-frame via
`.set_priority()`).

The pure blend/ease maths live in the new `sl-anim` `blend` module +
`Motion::pose_weight`/`is_finished` (unit-tested).

## P26.2 tree rendering — cross-cutting notes

- **Atlas textures bleed through alpha masks at UV seams under our REPEAT
  pipeline.** The viewer's object-texture path (`build_prim_image`) uploads
  every face texture with **repeat** addressing + bilinear filtering. The Linden
  tree texture is an **atlas** (trunk bark left half `u∈[0,0.5]`, leaf cards
  right `u∈[0.52,0.98]`) with transparent outer edges, so a face sampling
  exactly the atlas edge (`u=0`) repeat-wraps into the transparent far edge
  (`u≈1`); with `AlphaMode::Mask` that clips a thin see-through slit down one
  side of the trunk. Fixed by insetting the seam column's `u` a hair
  (`TRUNK_U_MARGIN` in sl-tree). **This will recur for P26.3 grass** (also an
  atlas-ish texture) and any future atlas — inset seam UVs, or give atlas
  textures clamp addressing.
- **Debug trick that isolated it:** temporarily set the tree material
  `AlphaMode::Opaque` — if a "gap" vanishes it is the alpha mask / texture, not
  geometry (the trunk cylinder ring is provably closed via a duplicate seam
  column, matching `LLVOTree`). The gap sits at a fixed angular position (the
  angle-0 seam, rotated by the 90° yaw), not a random hole.
- **`rez_sample_trees` example** (`sl-client-tokio/examples/`) plants a stand of
  species on OpenSim for viewer screenshots. OpenSim's
  `VegetationModule.AdaptTree` multiplies a rezzed tree's scale by **×8**
  (×(8,8,20) for cypress) on the UDP ObjectAdd path, so rez at a *small* scale
  (~0.5) or trees come out giant. Tree species byte rides the object **`state`**
  field (OpenSim + our decode agree).
- Tree size is driven by `scale.length() * 0.05` **uniform** (the reference
  `radius = getScale().magVec()*0.05`), NOT the per-axis object scale — a
  tree-specific geometry-holder transform in the viewer, distinct from the prim
  / mesh anisotropic scale holder.

## P26.3 grass rendering — cross-cutting notes

- **P26.3 grass DONE** (OpenSim-verified; design is in the roadmap/ viewer topic
  Done note). New: `sl-tree::grass` module + `GrassSpecies`/`GRASS_SPECIES`
  table + `sl-client-bevy::to_bevy_grass_mesh` + viewer `ObjectCategory::Grass`
  - `rez_sample_grass` example.
- **The P26.2 atlas-seam-bleed worry did NOT recur.** Grass uses
  `AlphaMode::Blend` (reference `PASS_GRASS`/`POOL_ALPHA`), not the tree's
  `Mask`, and the grass texture is a single blade sprite, not a bark+leaf atlas
  — so no `TRUNK_U_MARGIN`-style seam inset was needed.
- **Grass geometry depends on object scale** (blade-centre spread
  `x = exp_x * mScale`), unlike prims/trees whose scale only rides the
  geometry-holder. Handled by folding the X/Y scale (quantised to mm) into a
  **grass-only field of `ShapeFingerprint`**, so a resize rebuilds the clump
  through the *existing* geometry-rebuild path — no new LOD system/resource.
  Holder transform for grass is **identity** (scale already baked into the
  mesh).
- **OpenSim did NOT ×8-adapt grass scale** the way `VegetationModule.AdaptTree`
  does for trees: `rez_sample_grass` at scale 4 rendered a sensible few-metre
  clump (rez trees still need the small ~0.5 scale).
- **`.env` estate-owner login is currently broken** — `SL_ESTATE_OWNER_*` fails
  with `LoginRejected BadCredentials reason:"key"` on the local grid. The rez
  examples run fine as the **primary** avatar (avatar1); rezzing
  grass/trees is not permission-gated on the local standalone. (Reset the
  estate-owner password in OpenSim `auth.db` if an estate/owner test needs it.)
- **New R21 logged** (the roadmap/ viewer topic R section): a large flat
  **dark-blue plane** across the scene at the shoreline (likely the P23 water
  surface / underwater fog driven too dark or unlit) — spotted in the grass
  side-view screenshots, left for a focused water pass.

## P27.1 GLTF PBR materials — cross-cutting notes

- **P27.1 DONE** (design in the roadmap/ viewer topic Done note). New pure crate
  **`sl-material`** (decoder) + viewer `materials.rs` (fetch/apply). Placement
  choice: the glTF-material-*document* decode is its own crate (sibling of
  `sl-mesh`/`sl-texture`), separate from `sl-wire::material` which stays the
  *wire/cap codecs* (`GltfMaterialOverride` GenericStreamingMessage envelope,
  `RenderMaterials` cap, `ModifyMaterialParams`). `sl-wire::material` already
  existed but only handles overrides/legacy caps — it does NOT parse the glTF
  JSON document; that was net-new here.

- **Asset format gotcha:** the `AT_MATERIAL` asset is an **LLSD envelope**
  (`LLSDSerialize::LLSD_BINARY`, so bytes lead with `<? LLSD/Binary ?>`) of
  `{ version, type:"GLTF 2.0", data:<glTF-JSON string> }`; accepted versions
  `"1.0"`/`"1.1"`. The glTF `images[].uri` holds the **texture UUID** (not a
  path/data-URI), resolved via `textures[].source`. `sl-material` handles the
  binary + XML LLSD headers (mirrors `sl-mesh`'s header strip); needs
  `serde`+`serde_json` (new dep) for the glTF JSON.

- **Face→material link is `object.extra.render_material`**
  (`LLRenderMaterialParams`, extra-params type `0x80`, decoded into
  `RenderMaterialRef{face,material_id}` — already existed in sl-proto). NOT the
  per-face `TextureFace.material_id`, which is the **legacy `RenderMaterials`**
  id. Both existed decoded-but-unused before.

- **Integration approach that avoided heavy plumbing:** rather than thread the
  material refs + a `MaterialManager` through every geometry builder
  (`spawn_prim_faces`/`build_mesh_submeshes`/`build_sculpt_faces`) and every
  `Pending*` struct, attach an `ObjectRenderMaterials` component to the
  **geometry-holder entity** (the `ChildOf` parent of the face entities) in
  `apply_render_materials` (spawn+update), and a `register_pbr_materials` system
  joins each `Added<PrimFaceEntity>` face to its holder to discover the material
  id. One component + one query; no builder signature churn.

- **Colour-space split matters:** base-colour/emissive maps upload sRGB
  (`Rgba8UnormSrgb`), normal/metallic-roughness upload **linear** (`Rgba8Unorm`)
  — a separate `build_pbr_image(srgb)` (the shared `to_bevy_image` is
  sRGB-only). The decode (raw RGBA8) is shared with the diffuse pipeline via
  `TextureManager`; only the GPU image format differs, so `materials.rs` keeps
  its own `(TextureKey, srgb)` image cache. SL packs ORM in the
  metallic-roughness map (red=occlusion), so that one image is set on **both**
  `metallic_roughness_texture` and `occlusion_texture` (Bevy samples MR from
  G/B, occlusion from R).

- **Bevy single `uv_transform` limitation:** glTF allows a per-texture
  `KHR_texture_transform`; Bevy has one transform for all maps. We compose the
  **base-colour** transform onto the face's texture-entry placement (via
  `Affine2::mul`, a method not the `*` operator — arithmetic_side_effects) and
  let it stand in for every slot. Documented approximation.

- **PBR maps fetched boosted** (`TERRAIN_BOOST_PRIORITY`, full-res, not
  pixel-area LOD managed) — the render-priority driver ranks a face's *diffuse*
  texture, not the material maps behind it.

- **LIVE-VERIFICATION GAP (important):** neither reachable grid served a
  GLTF-PBR-material prim to render. OpenSim's Default Region has none; a fresh
  aditi login (primary avatar, park landing region) showed none in view and the
  `materialcache/` stayed empty. Both grids run the pipeline clean (no
  regression, screenshots normal), and the decoder is unit-tested, but an
  **on-screen PBR render is unconfirmed** — needs reachable PBR content (a
  provisioned OpenSim material, or an aditi spot with PBR builds). Also note
  material fetch uses the **ViewerAsset** cap, which the sl-conformance harness
  notes found persistently **503s on aditi** — so even with content, aditi may
  not fetch. Worth a follow-up.

- **Does NOT address R15** (aditi single-colour terrain): that is suspected
  **PBR *terrain*** (a terrain-material path), not prim/mesh **face** materials
  — a separate future item (P27.3 / terrain material work).

- **P27.2 GLTF material overrides DONE** (design in the roadmap/ viewer topic
  Done note). Non-obvious facts NOT in git/roadmap: (a) the P27.1
  LIVE-VERIFICATION GAP is now PARTLY closed on the OVERRIDE side — the aditi
  **park landing region actually pushes real overrides** (a single primary login
  logged `GLTF material override for object … on 4 / 1 face(s)`), so the
  GenericStreamingMessage→event→decode→apply path is live-exercised end-to-end;
  but the underlying base **maps still can't be shown** because aditi's
  `ViewerAsset` 503s (grey untextured buildings), so an on-screen
  *rendered PBR override* remains unconfirmed — the same ViewerAsset-503 wall as
  the asset/bake cases. OpenSim still serves no PBR/override content. (b) The
  override arrives as notation-LLSD (`LLSDSerialize::fromNotation`), which
  needed a brand-new **`sl_llsd::parse_llsd_notation`** (there was only the
  partial `Scan` cursor + binary/xml parsers before) — a full notation→`Llsd`
  parser, reusable for any future notation payload. (c) The reference AlphaMode
  enum order is **OPAQUE=0, BLEND=1, MASK=2** (NOT the 0/1/2 = opaque/mask/blend
  you'd guess) — the override's `am` integer uses it. (d) Viewer recompose
  gotcha: the face's diffuse `uv_transform` must be captured at
  face-registration and recomposed from (base_uv × base-colour KHR transform),
  else re-applying on an override double-stacks the transform; and a slot is
  only force-cleared when the *base* had a texture the override removed (else
  the P27.1 "no base map → keep diffuse" behaviour would regress). A `debug!` in
  `apply_material_overrides` logs each arriving override (object + face count) —
  the live signal used above.

- **P27.3 legacy (normal/specular) materials DONE** (design in the roadmap/
  viewer topic Done note). Non-obvious facts NOT in git/roadmap: (a) the ENTIRE
  wire/proto/runtime half was ALREADY built in a prior session (the `sl-wire`
  `LegacyMaterial`/`RenderMaterialEntry` zipped-binary-LLSD codec over the
  `RenderMaterials` cap, `Event::RenderMaterials`, the `RequestRenderMaterials`
  command + `run_render_materials_fetch` in both runtimes, both re-exports) —
  net-new was ONLY the viewer module `legacy_materials.rs` + its 5 systems in
  `main.rs`; a nice contrast to P27.1/P27.2 where the fetch was a per-asset
  `ViewerAsset` `AssetStore`. Legacy uses the cap's **batch** POST instead, so
  the manager is simpler (a material-id→waiting-faces queue, no
  `AssetStore`/LOD). (b) **The `RenderMaterials` cap WORKS on aditi** — unlike
  the `ViewerAsset` cap that 503s and left every asset/bake/PBR-map case
  aditi-partial. So P27.3 got a genuine end-to-end live-confirm the PBR cases
  never could: a single primary login drove
  **63 legacy materials requested = 63 received**, scene intact. This is the
  first Phase-27 case with a clean aditi round-trip of the actual material data
  (P27.1 had no content, P27.2 confirmed only the override path, base maps still
  walled by ViewerAsset-503). (c) **PBR supersedes legacy per-face** (reference
  behaviour): `register_legacy_materials` skips any face already covered by the
  object's `ObjectRenderMaterials` (P27.1) holder, so the two pipelines never
  fight over the same `StandardMaterial`. It reads the per-face legacy id
  straight off the existing `FaceTextureDebug.material_id` (the same "debug"
  component the P20 face driver reuses) — no new holder component needed. (d)
  **Mapping is normal-map- faithful, specular-scalar-approximate:** normal map →
  `normal_map_texture` (linear); `environment_intensity`→`reflectance`,
  glossiness (specular exponent) →`perceptual_roughness`; alpha mode maps only
  NONE→opaque and MASK→cutoff, leaving BLEND/EMISSIVE to the diffuse-derived
  mode (else a legacy material would shove opaque faces into the z-sorted
  transparent path). Bevy's `StandardMaterial` can't express the specular MAP
  texture (its `specular_texture` is `pbr_specular_textures`-gated +
  alpha-channel-reflectance semantics, mismatched) nor per-map (normal/specular)
  UV transforms, so both are dropped — a documented approximation like P27.1's
  single-`uv_transform`. **Phase 27 (PBR & legacy materials) DONE.**

- **P27.4 bump / shiny / glow / fullbright DONE** (green/no-panic OpenSim,
  live-confirmed on aditi; design in the roadmap/ viewer topic Done note). New
  viewer module `bump.rs`. Facts NOT in git/roadmap:
  - **The scalar three (fullbright/glow/shiny) apply INSIDE `face_material`**
    (textures.rs) via `apply_surface_flags(&mut material, face)`, the single
    chokepoint every face path already funnels through (prims, sculpts, meshes,
    rigged attachments, avatar body faces) — so no per-caller wiring and no new
    system for them. Only bump (needs the decoded diffuse) is a separate
    fetch/generate pipeline (`BumpManager` + `register_bump_faces` +
    `apply_bump_normals`, mirroring the P27.3 normal-map path).
  - **THE key gotcha — shiny must be an ANALYTIC-light highlight, NOT
    metallic.** The viewer has no reflection probe / `EnvironmentMapLight`
    (checked: the camera spawn in main.rs has no Bloom/Hdr/env map). A metallic
    `StandardMaterial` with nothing to reflect renders BLACK, so mapping
    shiny→`metallic` would make shiny faces look worse. Instead shiny raises
    `reflectance` + lowers `perceptual_roughness` (metallic stays 0), so the
    sun/moon `DirectionalLight` throws a sharper specular. Levels driven by the
    reference `SHININESS_TO_ALPHA = [0,.25,.5,.75]` (llface.cpp) as an
    environment-intensity ramp. Same "no bloom" reason glow is just an additive
    `emissive` (won't bloom, only reads brighter) — documented approximation.
  - **Bump generates a tangent-space normal map from a source texture's
    LUMINANCE** (Sobel central differences, wrapping to match the repeating face
    sampler), cached by `(TextureKey, invert)`. The SOURCE matches the
    reference: brightness / darkness derive from the face's own diffuse
    (darkness inverts); the 15 standard emboss codes (≥3)
    **fetch their fixed Linden bump texture** (`STANDARD_BUMP_TEXTURES`, the
    UUIDs from the reference's `app_settings/std_bump.ini` — woodgrain=3 …
    weave=17, built as consts via `Uuid::from_u128`) through the shared
    `TextureManager` and derive from that. The user asked to fetch the real
    assets rather than approximate them; live on aditi they DO exist and decode
    (saw woodgrain `058c75c0…`, gravel `4726f13e…`, siding `073c9723…` fetched +
    normal-mapped). Bevy's PBR shader synthesises tangents from UV derivatives
    when the mesh lacks a tangent attribute (prim/mesh faces have none), so
    normal maps work without generating tangents — same reason P27.3's legacy
    normal maps already worked.
  - **Precedence, mirroring P27.3:** `register_bump_faces` skips a face that has
    a PBR GLTF material (via the `ObjectRenderMaterials` holder) OR a legacy
    `material_id` (P27.3 supplies its real normal) — so bump only touches plain
    diffuse faces and the three normal-map writers never fight. Reads the bump
    code straight off `FaceTextureDebug.material_id`/`.bumpmap()` (the same
    "debug" component the P20 driver + P27.3 reuse) — no new holder component.
  - **Live: OpenSim Default Region has NO bump/shiny content** (renders clean,
    like P27.3's legacy faces), but an aditi landing region drove real bump —
    dozens of faces across many textures (6/8/16/112… per texture) generated
    normal maps with no panic (`RUST_LOG=sl_client_bevy_viewer::bump=debug`
    logs each). So aditi is the place to confirm, same as P20/P21/P27.2/P27.3.
  - **the roadmap/ protocol topic RENUMBERED this session** (user request): a
    NEW **Phase 28 — Animated textures** (`llSetTextureAnim`; `sl-proto`'s
    `decode_texture_anim`/ `TextureAnimation` already decodes the wire block, so
    it's a viewer-only driver) was inserted after Phase 27, pushing everything
    down one — animesh **29**, particles **30**, avian3d physics **31**, flexi
    **32**, reflection probes **33**, avatar cloth **34**, HUD **35**, aditi
    **36**. (So the phase list at the top of THIS file, lines ~62-71, is now
    off-by-one from Phase 28 on.) Also filed **R22** (avatars stay low-detail /
    blue spheres and never resolve on approach — avatar texture-LOD not managed
    - placeholder handoff).
  **Next open VIEWER item is Phase 28 (animated textures).**

- **P28.1 (ingest per-object texture animation) DONE** (design in the the
  roadmap/ viewer topic Done note: `texture_anim.rs` → `ObjectTextureAnimation`
  holder on the geometry-holder entity, `apply_texture_animation` beside
  `apply_render_materials`, `ON`-gated, `applies_to_face(u16)`). Facts NOT in
  git/roadmap:
  - **Ingest-only phases in a BINARY crate leave dead code** unless the state
    has a real consumer — the P27 material holders were used immediately by
    their register/apply systems, but P28.1's holder isn't read until the P28.2
    driver. `#[cfg(test)]` usage does NOT suppress `dead_code` in a normal
    (non-test) build, and `#[expect(dead_code)]` would trip
    `unfulfilled_lint_expectations` in the test build (same trap as the
    `too_many_arguments` note). The fix that keeps the phase self-contained AND
    live-verifiable: surface the state through the **`P` pick tool** (a real
    non-test consumer), exactly as P25.1 did for ingested lights. Reuse this for
    the ingest half of any future split phase (P30.1 particles, etc.).
  - **Pick-tool wiring detail:** the holder sits on the object's geometry-holder
    entity (not the object entity the pick walk-up lands on), so the pick reads
    it via `state.objects.get(&scoped).geometry` then `tex_anims.get(that)`. It
    also threads the picked face index down to report `applies_to_face`.
  - **OpenSim OAR recipe for a texture-animated prim** (Default Region had
    none): `<TextureAnimation>` is a **SceneObjectPart-level base64 element**
    (sibling of `<Shape>`, written after `<CollisionSoundVolume>` by OpenSim's
    `SceneObjectSerializer`), holding the raw 16-byte `TextureAnim` block
    (`encode_texture_anim` layout: mode u8, face i8, size_x u8, size_y u8, then
    3 LE f32 start/length/rate). `slclient-texanim.oar` (in `opensim/bin`,
    `mode=ON| LOOP`, `2x2` grid) provisioned + `load oar --merge`'d like the
    P25.1 light OAR. But it lands at 130,128,28 —
    **2 m inside the central prim wall**, so the ray hits a wall prim, not it
    (couldn't pick it). Verified the pick surface on **aditi** instead (a real
    `mode=0x13` `ON|LOOP|SMOOTH` scrolling prim); OpenSim confirmed only the
    ingest `debug!`. If P28.2 needs an easy OpenSim pick target, relocate that
    OAR clear of center (deleting the loaded copy first — the user declined a
    `delete object` on the live scene this session).

- **P28.2 (drive the animation) DONE** (design in the roadmap/ viewer topic Done
  note; visibly animated on aditi, user-confirmed). Facts NOT in git/roadmap:
  - **The animation REPLACES the face UV transform, it does not compose onto
    it** — this is the load-bearing correctness fact and NOT obvious from the
    roadmap prose ("compose an extra UV transform onto the face's texture-entry
    placement"). Firestorm's `LLFace::getGeometryVolume` uses
    `do_tex_mat = tex_mode && mTextureMatrix && !gltf_mat`: while an animation
    runs the face's `mTextureMatrix` is used **instead of** the static `xform`,
    not multiplied with it. The un-driven components fall back to the texture
    entry (`if !(result & ROTATE) rot=te; …TRANSLATE offset; …SCALE scale` in
    `LLVOVolume::animateTextures`), so the port derives an
    `AnimatedPlacement{rotation,offset,scale: Option<…>}` and fills each `None`
    from the face's `TextureFace` before building ONE `uv_transform`. So the
    driver writes `material.uv_transform = <fresh affine>` each frame, it never
    reads-modify-writes the existing transform.
  - **`mTextureMatrix` == the same `xform` affine.** I verified the llvovolume
    `tex_mat` (translate(-0.5)→rotate→scale→translate(off+0.5), applied
    row-vector `v*M`) is algebraically identical to `LLFace::xform` / our
    `texture_face_uv_transform`. So P28.2 needed NO new matrix math — I just
    factored `texture_uv_transform(rotation, off_s, off_t, scale_s, scale_t)`
    out of `texture_face_uv_transform` (both now share it, re-exported from
    sl-client-bevy) and feed it the animated params.
  - **Rust `%` on f32 IS C `fmod`** (truncated remainder, sign of dividend) —
    used directly for the reference's `fmod(frame_counter, full_length)` loop
    wrap and `fmodf(fc, sizeX)` cell selection. No libm / `rem_euclid` needed
    (they'd differ for negatives).
  - **Elapsed is accumulated per-frame, not read from a wall clock.** A
    `TextureAnimationClock{elapsed, anim}` component rides beside the
    `ObjectTextureAnimation` holder; the driver does
    `elapsed += time.delta_secs()` and restarts it (`elapsed=0`) when
    `clock.anim != holder.anim` (a fresh `llSetTextureAnim`). For BOTH the
    stepped and SMOOTH paths a constant-rate frame counter is `elapsed*rate`, so
    the reference's SMOOTH accumulator
    (`getElapsedTimeAndResetF32()*rate + mLastTime`) collapses to that — no
    separate accumulator kept. The `ObjectTextureAnimation` holder stays Copy /
    untouched by ingest; runtime state lives only in the sibling clock so a
    terse motion-update re-insert of the holder never resets the animation.
  - **Stop path uses `RemovedComponents`.** Turning the anim off in-world
    removes the holder (P28.1's `running_texture_animation` gate) but does NOT
    re-tessellate the prim, so the faces keep their last animated
    `uv_transform`. `restore_stopped_animations` reads
    `RemovedComponents<ObjectTextureAnimation>`, drops the clock, and resets
    each child face's `uv_transform` back to `texture_face_uv_transform(face)`
    (via the `FaceTextureDebug` `TextureFace`). Despawn is handled for free
    (`children.get` fails → no-op).
  - **The driver only reaches geometry-holder child faces** (prim faces, static
    mesh submeshes, sculpt, tree/grass) — all carry `FaceTextureDebug` +
    `PrimFaceEntity` + a unique `MeshMaterial3d<StandardMaterial>` per face.
    Worn rigged-mesh submeshes are parented to the AVATAR root (not the geometry
    holder) and lack `FaceTextureDebug`, so texture-animated worn rigged mesh is
    out of scope (edge case, matches the ingest holder's placement).
  - **OpenSim is a POOR visual test for P28.2** even though it ingests + drives
    fine: the `slclient-texanim.oar` prim's default texture is the synthetic
    placeholder `00000000-0000-1111-9999-000000000005` (no real asset), so the
    2×2 flipbook cell-stepping has no image content to reveal — it looks static.
    Decoded the OAR's base64 `<TextureAnimation>` to confirm rate=1/length=4 (it
    IS stepping). aditi's real-textured animated prims are the proper visual
    check. If an OpenSim visual test is ever wanted, re-provision that OAR prim
    with a real, quadrant-distinct texture. **Phase 28 (animated textures) DONE.
    Next open VIEWER item is Phase 29 (animesh).**

- **Phase 29 (animesh): P29.1 rendering DONE, P29.2 animation BLOCKED — pushed
  as 3 commits `e73e1c9`/`ff3ef3e`/`981419d`.** Design + the delivery-blocker
  findings are in the roadmap/ viewer topic P29.2 Done/blocked note; don't
  restate. Durable facts NOT in git/roadmap:
  - **`CompleteAgentMovement` is now deferred until caps are fetched — a
    cross-cutting change to EVERY login on BOTH runtimes.** `Session` defers it
    at `handle_login_response`; the driver calls new
    `Session::notify_capabilities_ready(now)` after its seed-caps fetch settles
    (bevy: in the `advance_running` map-channel drain; tokio: right after
    `fetch_capabilities`). On a cap-fetch FAILURE the driver **fails the login**
    (`fail_no_capabilities` + `is_awaiting_initial_capabilities`; tokio
    `Error::NoCapabilities{message}`, bevy caps channel is now
    `Result<map,String>`) rather than proceeding capless — the user chose
    fail-hard over a silent degraded session.
    **GOTCHA for any future session test:** a test that logs a client in must
    now call `notify_capabilities_ready` to release `CompleteAgentMovement` —
    the shared `lifecycle.rs::established` and `sim_session.rs::setup` helpers
    were updated, but a new bespoke login test will hang/assert-fail without it.
  - **`ObjectAnimation` is a CAPABILITY, not just a UDP message.** The sim only
    streams the `ObjectAnimation` UDP messages to a viewer that listed
    `ObjectAnimation` in its seed-caps request (`CAP_OBJECT_ANIMATION`, added to
    `REQUESTED_CAPABILITIES`); the cap is never fetched/POSTed, it's pure
    opt-in. Before adding it we got ZERO animation events on aditi. Same pattern
    likely applies to other "advertise support" caps.
  - **Why the Mario still doesn't animate (the real blocker):** the sim's
    `ObjectAnimation.object_id`s do NOT match the animesh we track/render. On a
    live aditi region ~15 of 17 animated objects were never tracked at all (an
    `ObjectAnimation` arrives but no `ObjectUpdate` ever does — most likely
    animesh **attachments on the coarse/distant avatars**, since the region had
    no fully-rendered neighbour avatars); the few tracked were linkset
    **children** with `animated=false`; and the in-world Mario roots we DO track
    get zero `ObjectAnimation`. So P29.2's driver is correct but never fed a
    matching event. **Next step is a wire capture** (`tcpdump` →
    `sl-conformance-trace`) to correlate `ObjectAnimation` vs `ObjectUpdate` ids
    — don't keep guessing from viewer-side logs. The animesh **rendering**
    LOD-race fix (wait on `MeshManager::lod_change_inflight` before binding a
    rigged mesh — an animesh is not an attachment so it starts on the coarse-LOD
    path and would freeze at a 4-vertex husk) IS confirmed live.
  - **503 retry:** transient 503/502/504 on GetTexture/GetMesh2/ViewerAsset now
    retried with exponential backoff (200ms→5s cap, 8 tries) via a new
    per-runtime `retry` module (`is_transient_status`/`transient_backoff`);
    texture+mesh fetchers had none, the asset fetcher's fixed-delay retry moved
    onto the shared helper.
  - **ggh commit gotchas hit this round** (the hook scans the WHOLE worktree,
    not just staged files): `typos` rejects certain hyphenated prefix compounds
    (write them closed, without the hyphen);
    `std::assert_eq!` is a disallowed macro (use `pretty_assertions::assert_eq`
    in every test module, incl. new ones); `cargo doc` fails on an intra-doc
    link to a no-longer-imported item (qualify it, e.g.
    `[\`PlayingAnimation\`](sl_client_bevy::PlayingAnimation)`).

- **Phase 30 (particles): P30.1 ingest + P30.2 simulate/render BOTH DONE**
  (skipped the still-blocked P29.2 animesh-animation per user). Design + live
  results are in the roadmap/ viewer topic P30.1/P30.2 Done notes; don't
  restate. Durable facts NOT in git/roadmap:
  - **The Bevy-free wire decode was ALREADY fully implemented+tested in
    sl-proto** (`particles.rs`: `decode_particle_system` → `ParticleSystem`,
    legacy+modern forms, round-trip encode too) long before P30.1, and
    `ParticleSystem` / `particle_pattern` were already re-exported from
    `sl-client-bevy`. So P30.1 was a pure **viewer ingest** — one new
    `sl-client-bevy-viewer/src/particles.rs` that is a near-verbatim mirror of
    the P25.1 light ingest (`lights.rs`): component + `*_from_object` lift +
    `apply_*` reconcile called from `apply_object`. Don't re-implement the
    decode. **P30.2 (simulate + render) already has everything it needs on
    `ObjectParticleSystem` (the object entity's component); it's the next open
    VIEWER item.**
  - **Particle content is aditi-only for testing.** The local OpenSim Default
    Region carries ZERO particle sources, so the ingest (and P30.2's render)
    can only be exercised on real SL — same "aditi is the proper visual check"
    pattern as Phase 28 animated textures. Live P30.1 run saw 9 in-view sources
    over 2134 tracked objects. Run the viewer on aditi with avatar `primary`
    (`--grid aditi --credentials credentials.aditi.toml --avatar primary`); MFA
    is automated by the credential file's `mfa_command` (yubikey `ykman` TOTP).
  - **P30.2 debugging lessons (the hard part was rendering, not the sim):** (1)
    a per-frame-rebuilt dynamic mesh MUST carry **`NoFrustumCulling`** — Bevy
    computes an entity's `Aabb` once when `Mesh3d` is *added* (from the
    then-empty mesh) and never recomputes it on an in-place mesh swap, so the
    cloud is culled from every viewpoint and renders NOTHING; `objects.rs` opts
    its rebuilt meshes out for the same reason. This will bite again for any
    future per-frame mesh (flexi, etc.). (2) When you can't hand-aim the camera
    at off-avatar content (a chronic pain on aditi),
    **decouple sim-correctness from render-visibility**: a throttled
    `RUST_LOG=…particles=debug` line
    (`particle sim: N cloud(s), M live particle(s)`) proved the sim was healthy
    (~2700 live across 28 sources) while the screen looked empty — so the bug
    was localised to rendering, not emission. (3) `SL_VIEWER_PARTICLE_FOCUS=1`
    snaps the fly-camera to the busiest cloud each frame — that's what finally
    framed the fountain jets (see the viewer headless debug-camera notes). The
    visual proof was a fountain rendering continuous upward billboard streams,
    NOT the "brief flashes" the earlier (culled + badly-framed) runs suggested.

- **Phase 31 (avian3d physics foundation): P31.1 integrate `avian3d` DONE**
  (skipped the still-blocked P29.2 animesh-animation per user). Design is in the
  the roadmap/ viewer topic P31.1 Done note; don't restate. Durable facts NOT in
  git/roadmap:
  - **Version pin: `avian3d 0.7.0` ↔ Bevy `0.19`.** avian 0.7.0's dep is
    `bevy ^0.19.0` (crates.io), the first avian that tracks our Bevy; avian
    churns its Bevy support per-release, so
    **bump avian in lock-step whenever Bevy is upgraded** (check the new avian's
    `bevy` req on crates.io first). Added to `sl-client-bevy-viewer` **only** —
    physics is a viewer render concern, no `sl-client-tokio` parity (same as
    sky/water/particles/materials).
  - **avian's *relative speed* IS a time-dilation control (its own docs say so),
    and that's how the region `TimeDilation` is applied.** The physics timestep
    is Bevy's shared `Time<Fixed>` delta × `Time<Physics>` relative_speed
    (`run_physics_schedule` in avian `schedule/mod.rs`), so: set `Time<Fixed>`
    at 45 Hz to pin the physics *rate*, and set `Time<Physics>` relative_speed
    per-frame = the agent region's dilation to *scale* it. `set_relative_speed`
    **panics on a negative / non-finite ratio** → the viewer clamps to
    `0.0..=1.0` (NaN→1.0) before setting. The user explicitly asked to fold in
    time dilation (the region sends `RegionData.TimeDilation` on every
    object-update; already surfaced as `Event::TimeDilation`, keyed to the root
    region via `SlIdentity::region_handle`).
  - **avian default features already carry what P31.2 needs.** Defaults include
    `default-collider` / `parry-f32` / `collider-from-mesh` (P31.2 builds
    colliders from prim/mesh geometry) + `parallel`; `debug-plugin` is on but
    `PhysicsPlugins::default()` does **not** add `PhysicsDebugPlugin` (add it
    separately if debug gizmos are ever wanted). Prelude import used:
    `avian3d::prelude::{Gravity, PhysicsPlugins, Physics, PhysicsTime}` — don't
    glob the prelude, it re-exports a `Vector` that collides with `sl_types`'
    `Vector`. **No visible change yet** (empty physics world until P31.2 gives
    server-flagged prims rigid bodies), so verification was build/clippy/unit
    tests + an OpenSim login smoke run (no panic/regression), not a visual
    check. **Next open VIEWER item is P31.2** (physical objects: avian rigid
    body + collider from prim/mesh geometry, sim stays authoritative).

- **Phase 31 P31.2 physical objects DONE** (still skipping the blocked P29.2
  animesh-animation per user). Design + verification are in the roadmap/ viewer
  topic P31.2 Done note; don't restate. Durable facts NOT in git/roadmap:
  - **How a physical prim was tested live** (there is no rez-prim console
    command and the default scene has none): built a throwaway
    `bin/slclient-physics.oar` (a `SceneObjectGroup` with
    `<Flags>Physics</Flags>`) and `load oar --merge`'d it
    **while the viewer was already logged in**, so the prim spawned and fell
    under the region's `ubODE` engine live (physics is currently ENABLED:
    `OpenSim.ini physics = ubODE`, not the disabled default). Load it AFTER
    login or it settles before the viewer streams it. The debug attach log is
    `physics.rs`'s `drive_physical_objects` None-branch
    (`RUST_LOG=…::physics=debug`). OAR + the fallen prim were cleaned up after.
  - **Kept the runtime-parity rule OFF** (viewer-only, like P31.1). All the
    dead-reckoning vector/quat math is per-component `f32`/`Quat`-method because
    the workspace `arithmetic_side_effects` lint forbids `Vec3`/`Quat` operators
    (see `camera.rs`'s comment); `indexing_slicing` is also denied so use
    array-destructuring, not `[i]`, in helpers.
  - **Circuit-liveness is a proxy**, not the real `LLCircuitData::isBlocked` —
    a `CircuitLiveness` resource timestamps the last inbound `SlEvent`; a
    healthy circuit streams steadily so it stays fresh, and only a genuinely
    stalled sim lets the phase-out engage. Good enough because a moving prim
    generates a stream of terse updates (fresh events) and a resting prim has
    ~zero velocity so never extrapolates anyway.
  - **Object deletion / OAR gotchas are now in the sl-client skill** (megaregion
    region-scope + `delete object id` not `name` + OAR regenerates UUIDs +
    query `OpenSim.db`) — see the sl-client skill, don't rediscover them.
  - **New open Phase-31 items split out on user request** (they were P31.2
    simplifications): **P31.3** physics-shape-aware colliders (fetch
    `GetObjectPhysicsData` → `LLPhysicsShapeType` none/hull/prim, replace the
    placeholder scale cuboid with a real geometry/hull collider) and **P31.4**
    avatar dead-reckoning (extend the interpolate-linear-motion port to
    `avatars.rs` with the stricter avatar land-height ground clamp). Then
    **Phase 32 flexi prims** (P32 is the first *client-only* dynamic-body user
    P31 was the foundation for). P29.2/3 animesh-animation still skipped.

- **Phase 31 P31.3 physics-shape-aware colliders DONE** (green live OpenSim,
  both the convex-hull AND prim/trimesh branches; still skipping blocked P29.2).
  Design is in the roadmap/ viewer topic P31.3 Done note. Durable facts NOT in
  git/roadmap:
  - **The entire wire/proto/runtime stack ALREADY existed** —
    `sl-wire/src/object_physics.rs` (`PhysicsShapeType` none/prim/hull +
    `ObjectPhysicsData` + the `GetObjectPhysicsData` LLSD codecs),
    `Command::RequestObjectPhysicsData`, BOTH `Event::ObjectPhysicsData` (cap
    reply, keyed by full `ObjectKey`) and `Event::ObjectPhysicsProperties`
    (event-queue push, keyed by `ScopedObjectId`), and
    `CAP_GET_OBJECT_PHYSICS_DATA`, all wired through both runtimes. **Net-new
    library was ONLY re-exporting `PhysicsShapeType` + `ObjectPhysicsData` from
    `sl-client-bevy` AND `sl-client-tokio`** (a latent parity gap — only the
    sim-features `PhysicsShapeTypes` plural was exported); everything else is
    viewer code.
  - **OpenSim only PUSHES `ObjectPhysicsProperties` on a physics-material
    CHANGE** (`SceneGraph.UpdateExtraPhysics` / material set), NOT on object
    stream-in, so a streamed physical prim delivers no unsolicited data — the
    **proactive `GetObjectPhysicsData` CAPS request is the reliable path**
    (OpenSim registers the cap in `BunchOfCaps.cs`). The viewer requests once
    per object on `Added<PhysicalObject>` (guarded by a `requested` set) and
    folds BOTH delivery paths into one `ObjectPhysicsShapes` keyed by full key
    (the push's `ScopedObjectId` translated via a new `ObjectState::full_key`).
  - **The OAR round-trips `<PhysicsShapeType>`** (`SceneObjectSerializer`), so a
    fixture pins the shape: added `bin/slclient-physics.oar` (shape **2**,
    ConvexHull) and `bin/slclient-physics-prim.oar` (shape **0**, Prim) — plain
    1 m `<Flags>Physics</Flags>` boxes. Unlike P31.2 (which needed
    load-AFTER-login for the live fall), a settled physical prim keeps
    `FLAGS_USE_PHYSICS` and persists to `OpenSim.db`, so
    **load-then-login also works** for the shape/collider check. Both fixtures
    left in the scene as reusable P31 test content.
  - **Collider geometry frame:** the avian collider lives on the object
    **root entity** (which carries NO scale — scale rides the geometry holder).
    So the collider is built from the object's own faces (found via a new
    `GeometryHolder` marker on the holder child, which
    **excludes linkset child prims** that also parent to the root) with each
    face vertex multiplied by the object scale — giving entity-local SL coords,
    exactly what the entity Transform then basis-changes, matching how the faces
    render. Do NOT apply the SL→Bevy basis change to collider points (the
    Transform does it). A 1 m cube yields **24 vertices** (6 quad faces × 4).
    `Collider::convex_hull(Vec<Vec3>)` / `Collider::trimesh(verts, indices)`;
    `Collider::aabb` needs the `SimpleCollider` trait in scope (unit tests).
  - **Collider ownership moved fully to `refine_physical_colliders`.**
    `drive_physical_objects` (P31.2) now only seeds the initial placeholder
    cuboid; all subsequent collider work (shape-aware build, resize rebuild, the
    `PhysicsShapeType::None` → no-collider removal) is refine's, recorded in a
    `RefinedCollider {shape, from_geometry, scale}` so it rebuilds only on real
    change and retries the geometry gather each frame until the meshes upload.
    These colliders are **inert** (kinematic movers) until P32/P34 add dynamic
    bodies — so verification is log-based (`::physics=debug`:
    `→ ConvexHull/Prim collider from N vertices`), the collider itself is
    invisible.

- **Phase 31 P31.4 avatar dead-reckoning DONE + P31.5 avatar movement controls
  DONE + P31.6 locomotion animations ADDED (open)** — all in one session. Design
  - verification are in the roadmap/ viewer topic P31.4/P31.5/P31.6 notes; don't
  restate. Durable facts NOT in git/roadmap:
  - **You cannot *observe* avatar dead-reckoning without driving the own
    avatar.** A fly-camera-only viewer never moves the agent, so the stationary
    own avatar reseeds to truth every frame and the P31.4 extrapolation never
    fires visibly. A solo/screenshot run only proves *integration* (the
    `avatar … → dead-reckoned` `::physics=debug` seed line, the stricter ground
    floor, no panic) — the extrapolation math itself is covered by unit tests.
    This is WHY movement controls (P31.5) were added mid-P31.4 (user-directed):
    they feed the dead-reckoner, and the user watched the own avatar
    walk/turn/fly **smoothly** live to confirm. **Movement is verified
    interactively (arrow keys) — it is NOT reproducible in screenshot mode** (no
    key input), so no automated live check.
  - **Movement plumbing was already complete end-to-end**
    (`Command::SetControls`/ `SetRotation`/`Autopilot`, `ControlFlags`,
    `session.set_controls`/`set_rotation` handled by both runtimes) — P31.5 was
    viewer-only (new `movement.rs` + registration). The sim
    **keep-alive re-sends the held controls** (`AGENT_UPDATE_INTERVAL`), so the
    viewer emits a command only on *change* (a `SetControls` on a flag change, a
    throttled `SetRotation` while turning). Turn keys track a
    **client-side heading** sent as the `AgentUpdate` body rotation (AT_POS
    walks along it), seeded once from the own avatar's reported facing to avoid
    a first-step snap; the sim does NOT yaw the avatar from the YAW control
    flags on its own — the body_rotation field is authoritative.
  - **Local OpenSim shows NO locomotion animation on the moving avatar; aditi
    DOES** (it has played standing anims in past runs) — this divergence is the
    P31.6 starting point (is OpenSim sending `AvatarAnimation` for these states
    at all, or is the received set not reaching the own-avatar play path?). The
    Phase 18 `AvatarAnimation` ingest/play pipeline is already proven on aditi.

- **Phase 31 P31.6 locomotion / state animations DONE + P31.7 turn-interpolation
  and P31.8 procedural-adjustment/adjuster overlay ADDED (open).** Full design +
  the two library bug-fixes are in the roadmap/ viewer topic P31.6 Done note;
  don't restate. P31.6 plays only the BASE keyframe — the reference viewer's
  procedural overlay (foot-skate/IK speed match, stand look-facing twist,
  head-track / eye / hand / breathe idle adjusters, editing-reach / targeting)
  is NOT ported → P31.8. Durable facts NOT in git/roadmap:
  - **The P31.5 open question is answered: local OpenSim DOES broadcast
    `AvatarAnimation` for the own avatar's locomotion** — the "no locomotion
    animation" was never a missing broadcast. `SL_VIEWER_LOG_LOCOMOTION=1` shows
    a clean `walk#n ↔ stand#n+1` stream ~0.4 s apart. OpenSim's
    `ScenePresenceAnimator.DetermineMovementAnimation` keys walk/stand off the
    **control flags** (`heldOnXY`), NOT velocity, and `SendAnimPack`s only on a
    state change; it serves the built-in locomotion `.anim` assets under their
    canonical UUIDs (`bin/assets/AnimationsAssetSet/index.xml`, e.g. `walk`
    `6ed24bd8…`), so they fetch over `ViewerAsset` on the local grid. The real
    blockers were both client-side: the `sl-anim` registry misclassification
    and the reconcile `stopped_at` timeline bug.
  - **The reconcile `stopped_at` bug was a LATENT P18.4 bug**, not new to P31.6:
    it only bit **looping** motions (a non-looping motion is saved by its
    natural ease-out `min(stopped_at, duration-ease_out)`), so every gesture
    always faded correctly and it stayed hidden until the registry fix made
    looping locomotion fetchable. Rule of thumb: a "stuck / never-fades"
    *looping* animation → suspect a wall-clock vs motion-elapsed `stopped_at`
    mismatch, not the wire.
  - **Presence decides whether the client fallback fires.** A root-presence
    login (this session was one) has the sim drive the avatar's own animations,
    so the fallback correctly stays deferred (logs `<simulator-driven>`). The
    child presence a megaregion login can land in is where it naturally fires.
    To exercise / verify the fallback on a **root** presence, set
    `SL_VIEWER_FORCE_CLIENT_LOCOMOTION=1` (skips the `has_active_sim_animation`
    deferral; client + sim states agree so the pose merge collapses to one
    anim).
  - **The fallback drives walk/run/turn from control-flag INTENT, not velocity —
    on purpose.** The first cut keyed walk off the P31.4 dead-reckoned
    `horizontal_speed`, which reproduced the "doesn't stop when you stop"
    symptom in miniature: the last-reported velocity lingers at walk speed and
    only decays over the ~2–3 s P31.2 phase-out (or never, if the sim goes
    silent), so the walk animation ran on for seconds after the key released.
    Intent clears the instant the key is up. Velocity is used only for fall /
    fly-vertical (no key).
  - **Verified interactively (user drives the arrow keys), NOT in screenshot
    mode** (no key input). xdotool key injection does **not** work on this
    Wayland desktop, so live movement checks need the user at the keyboard.

- **Phase 31 P31.7 interpolate avatar turning DONE.** Full design is in the
  the roadmap/ viewer topic P31.7 Done note; don't restate. Viewer-only, all in
  `physics.rs`. Durable facts NOT in git/roadmap:
  - **The premise held exactly:** the own avatar reports ~zero angular velocity
    (its facing is client-driven `SetRotation`, echoed back as sparse
    `ObjectUpdate`s), so the P31.4 `angular_step` never advanced rotation
    between updates and both `apply_object`'s update-frame snap and the
    between-update path wrote the authoritative facing straight onto the anchor
    → rotation stepped while translation eased. So this was NOT a
    dead-reckoning-rotation tuning issue, it was a *missing smoothing* issue.
  - **Fix shape:** `AvatarInterp` gained a `rendered_rotation` (Bevy space) that
    exponential-slerps toward the target each frame (`apply_smoothed_rotation` /
    `rotation_smoothing_alpha = 1-e^(-dt/τ)`, τ=80 ms); routed through the
    reseed AND both dead-reckon exit paths so it spans the update boundary.
    Chose slerp (general — smooths every avatar) over folding `movement.rs`'s
    client heading in (own-avatar-only + cross-module coupling). Converges
    exactly once turning stops, so no standing lag.
  - **Verified interactively on OpenSim** (user confirmed turning now as fluid
    as walking) — same keyboard-at-the-desktop constraint as P31.6.
  - **New open item P31.11 added on user request:** auto-stop flying on landing
    (SL clears fly state on ground contact; our P31.5 **F** toggle never
    self-clears, so a descent leaves the avatar hovering in fly mode). Live-
    observed gap, viewer-only, plumbing already exists.

- **Phase 31 P31.8 procedural motion adjustments DONE (as a deliberate SLICE;
  most of the item deferred to new P31.12–P31.15).** Full design + the
  ported-vs-deferred split are in the roadmap/ viewer topic P31.8 Done note and
  the new P31.12–P31.15 items; don't restate. Only the two
  **input-free always-on idle adjusters** landed — breathe (`mChest` sine) +
  body-noise (`mTorso` sway) — because everything else in P31.8 needs state this
  pass lacks: a world look-at target (head/eye), morph visual-params (blink), an
  IK solver (all locomotion foot-plant / stand-twist / fall / fly-bank), or
  selection state (editing / targeting). Durable facts NOT in git/roadmap:
  - **The idle adjusters are applied at POSE-APPLY time, not stored in
    `playback.poses`.** `pose_avatar_skeletons` clones the resolved keyframe
    pose (or default) per avatar and folds in
    `procedural::apply_idle_adjustments` before `deformed_world_matrices`.
    Deliberately NOT written back into `AnimationPlayback.poses`, because
    `drive_avatar_skeletons` OWNS and rebuilds that map from keyframes every
    frame and **edge-logs** pose start/stop — adding idle-only entries there
    would spam "released to rest" every frame and fight that ownership. So
    `apply_idle_adjustments(&mut pose, time, joint_index)` is the extension seam
    for the deferred P31.12–P31.15 adjusters too.
  - **Composition is `base.mul_quat(delta)`** (a small delta on the joint's
    existing keyframe rotation, so a playing animation still dominates); this
    needed `AnimationPose::{rotation,position}` made `pub` in `sl-client-bevy`
    (were private). `mul_quat`, not `*`, for the `arithmetic_side_effects` lint.
  - Viewer-only (no runtime parity), like the P31.6 locomotion fallback; pure
    math unit-tested in `procedural.rs`, not live-verified (the sway is subtle).

- **Phase 31 P31.12 head & eye look-at DONE (rotation only; blink split out).**
  New `look_at.rs` module (not `procedural.rs` — it needs cross-frame state and
  resources the input-free idle adjusters don't). Full design is in the roadmap/
  viewer topic P31.12 Done note; don't restate. Durable facts NOT in
  git/roadmap:
  - **The whole port runs in the avatar-local Second Life frame** (root at
    identity, `+X` forward, `+Z` up) — the same frame `deformed_world_matrices`
    produces — so the reference's world-space head math drops its
    `* ~rootRotWorld` inverse-root term and becomes a direct basis build. The
    one thing that needs the Bevy world is turning a look-at *target point* into
    a *direction in that local frame*: `root_global.rotation()` (the
    `AvatarAnchor` global, which already carries the SL→Bevy basis change) maps
    local→Bevy, so its inverse maps the Bevy head→target vector back. This
    sidesteps needing the avatar's SL object rotation at all.
  - **The pose pass now runs `deformed_world_matrices` TWICE for an avatar that
    has a look-at target** — once to read the head/eye joint world positions the
    aim needs, then again after folding the head/eye rotations. Untargeted
    avatars still run it once (the eyes only jitter, needing no positions), so
    the extra cost is paid only while actually gazing.
  - **Eyes are folded as local joint rotations on `mEyeLeft`/`mEyeRight` (+ the
    `mFaceEyeAlt*` pair when present); the eyeballs are rigid parts re-placed
    from their eye joint's posed matrix, so the fold rotates them for free.**
    The eye jitter is always-on (every rendered avatar's eyes drift), matching
    the reference; only the *aim* + vergence gate on having a target.
  - **The aim REPLACES the neck/head keyframe and is computed against the
    animated spine's ACTUAL world rotation — two live-testing fixes.** First cut
    folded the aim as `keyframe · delta` like the idle adjusters → the gaze
    **drifted back to the animation's head angle every idle-loop cycle** (the
    delta rode the looping head keyframe). Second cut replaced head/neck/torso
    but computed their locals assuming a *rest* spine → with an animation
    playing, the intermediate `mChest` keyframe (never accounted for) threw the
    head off the target entirely → **no visible tracking at all**. Final design:
    aim only `mNeck` + `mHead`, and derive each joint's *local* from its
    **parent's actual current world rotation** read out of the pre-fold deformed
    pass (`world0`, which already has the animation + idle folded in) —
    `neck_local = neck_parent_world⁻¹ · neck_world`,
    `head_local = neck_world⁻¹ · aim` — so the head lands exactly on the aim and
    *holds*, whatever the animation does to the spine. The reference reads
    `getWorldRotation()` live for the same reason. The reference's small
    **torso lag is dropped**: driving `mTorso` would invalidate the `world0`
    parent-world read for the neck (its `mChest` ancestor would have moved), so
    `mChest`/`mTorso` are left to the animation. Aiming still `slerp`s
    keyframe→absolute by an eased per-avatar `weight`
    (`LOOK_AT_WEIGHT_HALF_LIFE`), 1 while a target exists and easing to 0 after,
    so idle head motion returns smoothly. Consequence: the own avatar (always
    has the camera target) always head-tracks, overriding idle head motion —
    matching the reference's head-track priority.
  - **The rotation LIMIT is applied relative to the animated upper body, not the
    rest pose (third live-testing fix).** Clamping the aim to ±72° from *rest*
    forward looked lopsided — an idle stand animation that turns the body a few
    degrees made the head turn far over one shoulder but stop short the other.
    So `head_target_rotation` now returns the aim **unconstrained** and
    `apply_to_pose` clamps it in the neck-parent's frame
    (`neck_parent_world · constrain(neck_parent_world⁻¹ · raw_aim)`) — exactly
    why the reference constrains `targetHeadRotWorld · ~currentRootRotWorld`
    rather than the world aim. Now the ±limit is symmetric about wherever the
    body currently faces.
  - **Own-avatar look-at targets the fly-camera's own position** (the avatar
    tracks / makes eye contact with the viewer's camera) since the free-fly
    camera has no mouselook / cursor-focus analog for the reference's
    `LookAtPoint`. This was chosen over "aim along the camera forward" after
    live testing: forward-aim turned the head a lot (`head_angle` ~1 rad in the
    logs) but tracked a point *past* the avatar, so from behind one's own camera
    it read as no/ambiguous motion; looking *at* the camera makes the head+eyes
    visibly follow the viewer as you orbit. Others' look-at comes from the
    `ViewerEffect` `LookAt` (already surfaced as
    `SlSessionEvent::ViewerEffect`); its `GlobalCoordinates` target is placed
    with the agent region's SW corner as the scene origin (as terrain does), and
    targets expire after `LOOK_AT_TARGET_TTL` so a stale gaze relaxes to rest.
  - **Saccades use a per-avatar deterministic SplitMix64 PRNG** seeded from the
    agent uuid (no `rand` dep, no global RNG) — the reference's `ll_frand` table
    is itself re-seeded every startup, so only the *character* matters. `f32` in
    `[0,1)` is drawn cast-free via a 16-bit `u16::try_from` split (the
    `no as`-conversions rule).
  - **Added `ViewerEffect`/`ViewerEffectData`/`ViewerEffectType`/`LookAtType`
    to the `sl-client-bevy` re-export** (were reachable in `sl-proto` but not
    the Bevy façade the viewer imports from).
  - **Blink deferred as a real prerequisite chain, not hand-waved:** it drives
    `Blink_Left`/`Blink_Right` morph visual-params every frame, which the
    appearance pipeline (bakes morphs into geometry once) cannot do. Split into
    P31.12a (per-frame morph pipeline — also what P34 body-physics bounce needs)
    and P31.12b (the blink timer itself, `Saccade` is its future home).
  - Viewer-only (no runtime parity); the pure math is unit-tested in
    `look_at.rs`. **Live-verified** via `SL_VIEWER_LOOK_AT_TEST=1` (forces every
    avatar's head to crane ~72° left so the fold is unmistakable) +
    `SL_VIEWER_LOG_LOOK_AT=1` (per-avatar target / applied head-angle /
    head-joint log): heads turn and eyes shift, `mHead` resolves, and the own
    avatar shows `target=true` (the camera path sets it). The **real**
    own-avatar turn is invisible when the camera looks forward past the avatar
    (look dir ≈ `+X` → identity) — aim the camera to a side to see it; idle
    neighbours send no gaze, so `target=false` for them is correct, not a bug.

- **Phase 31 P31.13 hand-pose morph DONE.** Full design is in the roadmap/
  viewer topic P31.13 Done note; don't restate. Durable facts NOT in
  git/roadmap:
  - **The hands are a MORPH, not a pose.** This is the thing to internalise
    before touching finger rendering: no `.anim` keyframe track ever drives the
    finger joints. Each `.anim` header carries a hand-pose *index*, and the
    viewer turns that into one of thirteen `Hands_*` visual-param morphs on the
    **upper-body** mesh (all thirteen live in `avatar_upper_body.llm`, confirmed
    by its morph names). So the hand shape rides the appearance/morph pipeline,
    not the animation pose pipeline — which is exactly why it had to wait for
    P31.12a.
  - **`mMaxPriority` exists only for hand poses.** It looked like a general
    motion priority; it is not. Grepping the reference,
    `LLJointMotionList::mMaxPriority` is read in exactly one place —
    `applyKeyframes`'s hand-pose publish. It is also NOT the motion's base
    priority: it starts at `LOW` and only *explicit* joint priorities lift it,
    so a `HIGHEST`-base animation whose joints all say `USE_MOTION` arbitrates
    hand poses at `LOW`. Hence `Motion::max_priority()` in `sl-anim` rather than
    reusing `base_priority` or the per-joint `effective_priority`.
  - **The reference's hand-pose bounds check is off by one, and we keep the bug
    on purpose.** `LLKeyframeMotion` rejects a header hand pose only when it is
    **above** `NUM_HAND_POSES` (14), so an index of exactly 14 decodes fine —
    and `LLHandMotion` then re-checks with `< NUM_HAND_POSES` and *ignores* the
    request (leaving the hands heading wherever they were, which is NOT the same
    as the "no request" branch that relaxes them). Our decoder already mirrored
    the loose check, so `HandPose::is_known()` + the ignore branch reproduce the
    other half.
  - **Priority ties break the OPPOSITE way from the pose blend.** The
    active-motion list is newest-first, and the hand-pose guard is `>=` (so the
    last-visited, i.e. *oldest*, motion wins a tie) while joint blending is `>`
    (so the first-visited, *newest*, wins). Both are faithful; don't "fix" the
    asymmetry.
  - **Cost note:** every part carrying a runtime morph gets *dense* Bevy morph
    targets, so the upper body now uploads 13 targets × its vertex count per
    avatar (the head has 2 for blink). Fine at current avatar counts, but this
    is the thing that grows if more params join `RUNTIME_MORPH_PARAMS` (P34 body
    physics is next).
  - **Visible baseline change:** the resting pose is `HAND_POSE_RELAXED`, not
    the base mesh's `HAND_POSE_SPREAD`, so every avatar's hands are now relaxed
    rather than splayed even with no animation playing.
  - Viewer-only (no runtime parity), like the other P31 adjusters.
    **Live-verified on OpenSim**: `SL_VIEWER_HAND_POSE_TEST=<index>` forces a
    pose on every avatar, `SL_VIEWER_LOG_HAND_POSE=1` traces the transitions.
    Note the system-avatar hand morphs are geometrically WEAK — a forced
    `Hands_Fist` reads as "curled fingers", not a tight fist. That is correct
    (the base mesh's fingers are very low-poly and the reference looks the
    same); do not chase it as a bug. The animation-driven path is easiest to
    exercise with the P31.9 `T` typing toggle, whose `ANIM_AGENT_TYPE` header
    requests `Hands_Typing`.

- **Phase 32 P32.1 ingest flexible-object data DONE** (skipping the blocked
  P29.2 animesh + the rest of P31 per the user). Full design is in the roadmap/
  viewer topic P32.1 Done note; don't restate. Viewer-only (no runtime parity —
  `FlexibleData` was already re-exported from BOTH runtimes, no gap), a straight
  mirror of the P25.1 light / P30.1 particle ingest (new `flexi.rs`:
  `ObjectFlexi` component + `flexi_from_object` lift + `apply_flexi` reconcile
  on both `apply_object` paths). Durable facts NOT in git/roadmap:
  - **New live test fixture: `slclient-flexi.oar`** (in
    `~/devel/3rdparty/opensim/bin`, NOT tracked in the repo — that's a 3rd-party
    tree, like all the other `slclient-*.oar`). OpenSim's Default Region carries
    NO flexi content, so it was hand-built like the P25.1 light OAR.
    **OAR recipe for a flexi prim:** in the `SceneObjectPart`'s `<Shape>` set
    `<FlexiEntry>true</FlexiEntry>` plus the value fields `<FlexiSoftness>`
    (0–3) / `<FlexiTension>` / `<FlexiDrag>` (0–10) / `<FlexiGravity>` (−10..10)
    / `<FlexiWind>` (0–10) / `<FlexiForceX/Y/Z>` — OpenSim REGENERATES the
    `<ExtraParams>` blob from these on serialize, so a viewer `ObjectUpdate`
    then carries the `PARAMS_FLEXIBLE` extra param. Built the OAR as a tall thin
    cylinder (Scale 0.3,0.3,4, Circle profile) at the Default Region landing
    (128,128,26) so P32.2 has something that visibly droops. Load with
    `load oar --merge …/slclient-flexi.oar`.
  - **Live-verified the ingest headless** (no user needed): the object streams
    to the agent's interest area on login regardless of camera aim, so
    `RUST_LOG=info,sl_client_bevy_viewer::flexi=debug` + a screenshot-dir run
    (auto-quit) logged `object flexi prim:` with `softness=2 tension=1.00
    air_friction=2.00 gravity=0.30 wind=0.00 user_force=(0,0,-0.50)` — the exact
    values set in the OAR. `air_friction` is the on-wire "drag":
    `FlexibleData` names it `air_friction`, OpenSim's field is `FlexiDrag`.
  - Flexi is mutually exclusive with server physics (reference forces phantom +
    non-physical), so a flexi prim never carries the P31.2 physics-body marker.

- **Phase 32 P32.2 simulate flexi DONE** (green OpenSim, user + screenshot
  confirmed a thin cylinder drooping into a smooth arc). Full design in the the
  roadmap/ viewer topic P32.2 Done note (the metre-bake / identity-holder
  architecture, the pure `sl-prim::flexi` `FlexiChain` solver,
  `tessellate_with_path`, the `simulate_flexi` ECS glue) — don't restate. Facts
  NOT in git/roadmap:
  - **The live bug the user caught (and its lesson).** My first cut kept the
    flexi geometry **unit-local** and let the per-object scale holder re-apply
    the prim scale (like a rigid prim). That is CORRECT for positions (the
    division is the holder's exact inverse) but WRONG for the swept profile: the
    holder's non-uniform scale is applied *after* the section rotation, so a
    bent section's circular cross-section gets sheared — for a `0.3×0.3×4 m`
    flexi cylinder (13× aspect) it ballooned into a giant slab the moment it
    drooped. Fix = the reference's approach: bake **full-metre** geometry (prim
    X/Y scale into the profile *before* the rotation) and give the flexi prim an
    **identity geometry holder** (the grass pattern — `holder_transform` returns
    IDENTITY when `object.extra.flexible.is_some()`). GENERAL LESSON: never let
    a non-uniform Bevy transform scale a mesh whose per-vertex frames rotate
    (flexi, and any future bend/deform) — bake the scale into the geometry
    before it.
  - **The prim's live scale rides the `ObjectFlexi` component**, refreshed every
    update (`flexi_from_object` now also stores `object.scale`), because the
    flexi holder is identity (carries no scale) — the sim reads it there so a
    resize stays correct.
  - **New live fixture `slclient-flexi-h.oar`** (in
    `~/devel/3rdparty/opensim/bin`): a `0.3×0.3×4 m` flexi cylinder rotated 90°
    about Y (laid horizontal) at region-local (128,132,26), gravity 3 / softness
    3 — the vertical `slclient-flexi.oar` from P32.1 does NOT visibly bend
    because its gravity + user-force are both AXIAL to a vertical cylinder (a
    vertical flexi under axial-only forces correctly stays straight — the length
    constraint absorbs it); a horizontal prim makes gravity lateral, so it
    droops visibly. Camera
    `--camera-position 128,138,26 --camera-look-at 128,132,24.5` frames it
    side-on.

- **R22b ("blue spheres never resolve") — CLOSED, NOT A BUG.** Root cause found
  live on aditi: the test parcel had the About-Land option
  *"Avatars on this parcel can see and chat with avatars on other parcels"*
  **unchecked**, so the region deliberately withholds other-parcel avatars'
  object data (radar/minimap only, i.e. our coarse sphere) — a Second Life
  privacy feature, not a client fault (Firestorm shows the same spheres on such
  a parcel). Fits the telemetry exactly: every unresolved sphere had
  `ever_full_object=false` and only the avatar on our own parcel rendered;
  camming to ~6 m of a sphere never streamed it (camera is irrelevant when the
  sim withholds by policy). So DON'T chase this as an interest-list/streaming
  bug again — check the parcel privacy setting first. The investigation still
  produced three genuine Firestorm-parity omissions that were fixed+pushed and
  kept (they do NOT affect this parcel-privacy case): commits `c4d9f83` report
  the interest camera even in fixed-camera/`--camera-position` mode (was gated
  on the login camera-snap); `a63a1f5` send `AgentHeightWidth`/`AgentFOV` (the
  `SetAgentSize`/`SetAgentFov` plumbing already existed in both runtimes, viewer
  just never called it); `e63d975` send `AgentThrottle` (`Throttle::preset_1000`
  at region handshake — we sent NONE). Plus diagnostics `c6d8cd9`. **R24 DONE**
  (doc `bc80c95`, impl `43c7b7d`): child-circuit `CoarseLocationUpdate` was
  dropped in `dispatch_child` so neighbour-region avatars got no coarse dot; now
  root+child both build the event via a shared `coarse_location_event` helper
  tagging it with a new `region_handle` field on `Event::CoarseLocationUpdate`,
  the viewer offsets a neighbour's dots by `region−origin` metres (shared
  `coords::metres_to_f32`) and reconciles coarse dots PER region, and
  `DisableSimulator` emits an empty update for the retiring region so its dots
  drop rather than linger. Permanent diagnostics (5 s coarse census + per-avatar
  distance name tags + per-arrival full-object log + reported interest
  camera/viewport) sit behind `SL_VIEWER_LOG_AVATAR_INTEREST=1`.

- **Phase 33 P33.1 default (global) reflection probe DONE** (structure/behaviour
  in the roadmap/ viewer topic P33.1 — don't restate; local per-object probes
  split to P33.2, brightness calibration to P33.3). Non-obvious Bevy-0.19
  integration facts and open issues NOT in git/roadmap:
  - **Bevy has the sink, not the source.** `GeneratedEnvironmentMapLight{cube,
    intensity,rotation}` (bevy_light, in DefaultPlugins via `LightProbePlugin`)
    takes a source cube `Image` and **re-filters it EVERY frame** (SPD mips +
    Lambertian/GGX) into an `EnvironmentMapLight` — so it's a per-frame GPU cost
    regardless of capture. There is NO built-in scene→cubemap capture; we supply
    it (six 90° `Camera3d` + `Hdr` + `Tonemapping::None` + `Msaa::Off`).
  - **RenderTarget::Image + a render-world copy, NOT ManualTextureViews.**
    Rendering a camera straight into one cube array layer needs a per-face `D2`
    `TextureView` (`base_array_layer=face`) in the render-world
    `ManualTextureViews` — but that's an `ExtractResource` and the MAIN-world
    `camera_system` computes a `RenderTarget::TextureView` camera's
    `target_info` from the MAIN-world copy, where a render-world-only view can't
    be built → `extract_cameras` skips the camera. Dead end. Instead: 6 plain
    `Rgba16Float` image targets (camera sizing "just works" from
    `Assets<Image>`) + a render-app system (`Render`,
    `.after(RenderSystems::Render)`) that makes its OWN `CommandEncoder`
    (`RenderContext` only works beneath the graph schedule) and
    `copy_texture_to_texture`s each face into the cube's 6 layers; the filter
    reads the cube a frame later (harmless lag, capture is slow).
  - **Face orientation: use `bevy_camera::primitives::CUBE_MAP_FACES`.**
    Verified it matches the env-map sampler's convention EXACTLY (incl.
    up-vectors and the non-standard +Z/−Z layer swap):
    `Transform::looking_to(face.target, face.up)` per layer reproduces
    `sample_cube_dir(uv,face)` + the left-handed z-flip that the atmosphere path
    bakes in. Don't hand-roll the 6 view matrices.
  - **FPS: amortize the capture.** Six full scene re-renders — each with its own
    directional-shadow cascade pass — tanked FPS to single digits when done
    every frame. `CAPTURE_PERIOD_FRAMES` runs the 6 faces as a brief burst a few
    times a second, then idles; that alone restored ~baseline FPS (the per-frame
    filter is not the dominant cost).
  - **Custom materials sample the env map for consistency** (Bevy only lights
    `StandardMaterial`). terrain.wgsl/water.wgsl `#import mesh_view_bindings`
    and, under `#ifdef ENVIRONMENT_MAP`, sample
    `diffuse_/specular_environment_maps[ u32(light_probes.view_cubemap_index)]`
    (the `MULTIPLE_LIGHT_PROBES_IN_ARRAY` binding-**array** form is the one
    active on desktop — the singular `diffuse_environment_map` is NOT in scope,
    that was the first WGSL error) with a local `quat_rotate(view_rotation,dir)`
    - `dir.z=-dir.z`. Sky stays the source (not probe-lit). To avoid
  double-counting a flat ambient on the probe's diffuse IBL,
  `suppress_global_ambient` drops the sky-set `GlobalAmbientLight` in
  PostUpdate.
  - **RESOLVED by P33.3** (this entry's "no visible change between
    `SL_VIEWER_PROBE_INTENSITY=400` and `4000`" was a **bad measurement**, not a
    Bevy bug — `intensity_for_view` does track
    `GeneratedEnvironmentMapLight.intensity` exactly as documented; see the
    P33.3 entry below).
  - **Screenshot mode now logs out cleanly** (net-new general fix):
    `capture_ screenshots` calls a shared `session::request_logout`
    (Command::Logout + quit deadline) instead of `AppExit` — an abrupt process
    exit stranded the grid avatar session and blocked the NEXT login (empty logs
    / "no viewer started").
  - **Op gotcha:** launching the viewer FOREGROUND from the agent's Bash tool
    fails (sandbox/GPU); background launches or the user's manual launch work. A
    leading `pkill` returning 1 (no match) aborts the whole compound command —
    use `pkill … || true`.

- **Phase 33 P33.3 brightness calibration DONE** (what it *is* — the
  exposure-derived unit gain, the HDR view, the reference tone mapper, why probe
  **ambiance** is not expressible — is in the roadmap/ viewer topic P33.3; don't
  restate). What is not in git/roadmap:
  - **The two symptoms had one cause, and both were visible in a mirror-ball
    capture**: the terrain read a *different colour* in the ball than in the
    world (before: direct `(141,161,182)` blue-grey vs `(90,86,63)` in the ball;
    after: `(82,88,72)` vs `(65,64,42)` — same family), and the scene read "a
    bit bright". Cause: no `Hdr` on the main camera ⇒ Bevy's `TONEMAP_IN_SHADER`
    path tonemapped `StandardMaterial` in the mesh shader while the custom
    sky/terrain/water materials were merely clipped by the 8-bit target.
    **The mirror ball is the calibration instrument** — a probe is calibrated
    exactly when what it shows of a surface matches that surface beside it.
    Reach for it first on any future lighting change.
  - **Capture cameras must be lit by the probe too** (`light_capture_cameras`):
    they are ordinary views, so Bevy gives them no IBL of their own, and with
    the flat `GlobalAmbientLight` suppressed the whole captured world came out
    ambient-less (black shadowed sides, terrain on its no-probe fill) — a cube
    visibly darker than the world beside it. Share the main view's
    *already-filtered* `EnvironmentMapLight` handles; adding a
    `GeneratedEnvironmentMapLight` per capture camera instead would start a
    filter chain running per camera. The resulting frame-to-frame feedback is
    the point (bounced light) and converges on surface albedo.
  - **The P33.1 "`intensity_for_view` doesn't respond" measurement was an
    artifact of the headless run**, not a Bevy bug. Unfocused/occluded, the
    viewer's window drops to ~1 FPS; `Time<Virtual>`'s 250 ms `max_delta` then
    makes app-elapsed run ~4× slower than wall clock, and the *frame*-counted
    `CAPTURE_PERIOD_FRAMES` (180) means the first cube refresh after login lands
    minutes late — so a screenshot taken on a fixed delay catches a
    **half-black, stale cube** and any A/B across it is meaningless. Check `FPS`
    in the diagnostics overlay of a capture before trusting it: 60 = valid, 1 =
    the cube is stale.
  - Tone-mapper A/B knobs land as `SL_VIEWER_TONEMAP` (`aces`/`neutral`/`none` —
    `none` reproduces the old clipped look), `SL_VIEWER_TONEMAP_MIX`,
    `SL_VIEWER_EXPOSURE`; `SL_VIEWER_PROBE_INTENSITY` is **gone**, replaced by
    the dimensionless `SL_VIEWER_PROBE_GAIN` (1 = calibrated).

## P34.1 body-physics ingest — cross-cutting notes

- **The physics `*_Driven` morph targets exist in NO `.llm` file.** The eight
  driven params (`Breast_Physics_UpDown_Driven`, …) that `avatar_lad.xml` names
  have no morph data in any base mesh — `strings avatar_upper_body.llm` finds
  none. The reference viewer *manufactures* them while loading each part
  (`LLPolyMeshSharedData::loadMesh`, `indra/llappearance/llpolymesh.cpp`), by
  cloning a shape morph that already moves the right vertices:
  - `Breast_Gravity` → `Breast_Physics_UpDown_Driven`
    (`clone_morph_param_duplicate` — the source deltas verbatim);
  - `Breast_Female_Cleavage` → `Breast_Physics_InOut_Driven` (duplicate) **and**
    `Breast_Physics_LeftRight_Driven` (`clone_morph_param_cleavage`, scale 0.75
    with the **Y sign mirrored** on the deltas that already point at −Y, so both
    breasts sway the *same* way instead of towards each other);
  - `Big_Belly_Torso` / `Big_Belly_Legs` / `skirt_belly` / `Small_Butt` → the
    belly and butt targets (`clone_morph_param_direction`, which throws the
    source deltas away and replaces every one with a single constant
    displacement — the source morph only selects *which* vertices move, and
    donates their UV shifts).
  `BaseMesh::from_bytes` now does the same (`synthesize_physics_morphs`). Without
  this the driven params are silently inert: `LLPolyMorphTarget::setInfo` looks
  the morph data up by param name (stripping a `_Driven` suffix as a fallback),
  and our own `MorphWeights::apply` simply finds no target of that name.

- **A `<param_morph>` may also displace COLLISION VOLUMES.** Its
  `<volume_morph name="LEFT_PEC" scale="…" pos="…">` children add
  `weight * scale` / `weight * pos` to a volume's rest transform
  (`LLPolyMorphTarget::apply`'s volume pass). Since P17.2 the collision volumes
  *are* bindable joints, so this — not the mesh morph target — is how a worn
  **rigged mesh** body bounces or follows a shape slider. `ParamEffect::Morph`
  therefore carries `Vec<VolumeMorph>` now. Note the reference runs the volume
  pass **only when the param's morph data exists on that part** (the
  `!mMorphData` early return), so a volume morph applies once per part carrying
  the morph, not once per param. The ~30 *shape* params that carry volume morphs
  are parsed but still unapplied — see the P34.3 task.

- **The physics wearable configures a simulation, it does not shape the body.**
  Its transmitted params (ids 10000–10032) are pure spring-damper settings
  (mass / gravity / drag / spring / gain / damping / max-effect), one set per
  body part and axis; the six motions each write a **hidden controller** param
  (group 1, never transmitted, so it always resolves to its default → the
  bounce's rest position is the *middle* of the driven range), which drives the
  `*_Driven` morphs. `Max_Effect` defaults to **0** on every axis, i.e. physics
  is off unless the wearable turns it on — so `0 of 6 motion(s) active` is the
  expected ingest log for an ordinary avatar, and a live check of the *bounce*
  needs an avatar actually wearing a tuned physics wearable.

- The breast settings are `sex="female"`, so the ordinary sex gate in
  `ResolvedParams::effective_weight` already switches the breast motions off for
  a male avatar — no special case needed.

## P34.2 body-physics simulation — cross-cutting notes

- **Nothing bounces by default, so the reference ships a test switch — and so do
  we.** `Max_Effect` is zero on every axis unless a tuned physics wearable sets
  it, and the OpenSim test avatar wears none. `SL_VIEWER_PHYSICS_TEST=1` is the
  port of the reference's own `physics_test` bool (`behavior_maxeffect = 1.0f`):
  it forces every motion on at ingest, which is the only way to *see* the
  simulation without a wearable. `SL_VIEWER_LOG_BODY_PHYSICS=1` logs each
  avatar's per-motion simulated position (`0.5` = the user's own shape).
- **The bounce has two outputs, not one.** The `*_Driven` morph params move the
  **system body** (through the P31.12a runtime-morph pipeline); the same params'
  **volume morphs** move the `LEFT_PEC` / `RIGHT_PEC` / `BELLY` / `BUTT`
  collision volumes, and *that* is what makes a worn rigged-mesh body bounce —
  a system-body morph target cannot reach one. The volume displacements are
  folded in as `AnimationPose::set_position` deltas on the volume joints (the
  pose position track is already an offset from a joint's rest, which is exactly
  what a volume morph is), so they cost nothing extra: the same final
  `deformed_world_matrices` call picks them up.
- **The joint sample must include the avatar's own travel.** The forcing term is
  the *world* acceleration of `mChest` / `mPelvis` (the reference's
  `LLJoint::getWorldPosition`), so the sample is taken in **Bevy world space** —
  the avatar-local SL joint matrix composed through the avatar-root global.
  Sampling avatar-local would miss walking, jumping and landing entirely, i.e.
  everything that actually bounces. The SL → Bevy axis change is a proper
  rotation, so the 1-D projection along the motion axis is the same number in
  either frame; only world-up has to be named per frame (`+Y` in Bevy).
- **Two deliberate deviations from `llphysicsmotion.cpp`**: the first frame only
  seeds the joint trail (the reference's `mPosition_world` starts at the origin,
  so its second frame differentiates a *region-sized* jump and kicks the springs
  to their limits), and a degenerate mass makes a motion inert instead of
  dividing by zero (`a = F/m`; the wearable's slider bottoms out at `0.1`, but a
  table that omits the param need not).
- `pose_avatar_skeletons` hit Bevy's **16-system-parameter limit** with this
  fold; the procedural adjusters' resources (look-at, reach, locomotion, body
  physics, runtime morphs) are now bundled into one `AvatarAdjusters`
  `SystemParam`. Any further per-avatar fold should join that bundle.

## P34.3 shape volume morphs — cross-cutting notes

- **A param declared under several `<mesh>` parts is one table entry but several
  morph targets.** `VisualParams::from_xml` keeps the *last* declaration of a
  param id (the reference's `addVisualParam` map overwrite) — but the reference
  also builds one `LLPolyMorphTarget` **per `<mesh>` declaration**, each with
  that declaration's own `<volume_morph>` list, and `apply` walks the whole
  chain. The head params (`Squash_Stretch_Head`, `Elongate_Head`, …) are
  re-declared `shared="1"` under the **eyelash** mesh *without* volume morphs,
  and that volume-less declaration is the later one — so a last-wins table
  silently drops their `HEAD` displacement. `from_xml` now **concatenates** the
  volume-morph lists across declarations of one id, which both fixes that and
  reproduces the reference's multiplicity (a volume morph declared on two parts
  applies twice). Every other field still follows last-wins.
- **A volume morph is the *only* way a shape slider reaches a rigged mesh
  body.** The system body is skinned to the `m*` bones and takes its shape from
  the morph *targets*; a mesh body is rigged to the collision volumes, which no
  morph target can touch. So the ~30 shape params carrying `<volume_morph>`
  (`Big_Chest`, `Fat_Torso`, `Breast_Gravity`, `Bowed_Legs`, `Foot_Size`, …) are
  what make a fitted-mesh body follow the chest / belly / butt / leg / head
  sliders. `VolumeDeformations` (`sl-avatar::volume`) resolves them exactly like
  `SkeletalDeformations` resolves `param_skeleton`, and the Bevy skeletal
  recurrence adds them to the volume joint's rest scale / position. Bone names
  (`mChest`) and volume names (`LEFT_PEC`) are disjoint, so one pass over the
  joint list handles both.
- **The physics `*_Driven` params are excluded from that resolver** — they carry
  volume morphs too, but P34.2 applies theirs per frame as pose deltas, so
  counting them again in the rest transform would double the bounce. Their rest
  weight is zero anyway (the hidden controller params default to the middle of
  their range), so nothing is lost. The two therefore compose: the volume
  *rests* where the shape puts it and *bounces* around that.
- **A live A/B of anything that shapes an avatar must happen inside ONE
  session.** Two logins are never comparable: the sun has moved, the scene
  streams differently, and on a live grid the other avatars in the region are
  *different people*. Hence the `V` key (`VolumeMorphGain`, seeded from
  `SL_VIEWER_VOLUME_MORPH_GAIN`; `0` = pre-P34.3 rest volumes, big =
  exaggerated) — toggle the effect on one avatar and watch it change. Two
  supporting knobs: `SL_VIEWER_TPOSE=1` freezes every avatar at its shaped rest
  pose (an AO otherwise walks and turns it), and `SL_VIEWER_VOLUME_FOCUS` frames
  the avatar whose shape displaces its volumes most (`=1`) or a pinned agent id.
- **The agent's own shape is usually the WORST test subject.** A slim,
  near-default shape displaces its volumes by *nothing* — the aditi test
  avatar's `BELLY` takes a zero position delta — so amplifying the effect 8×
  still shows nothing, and it looks like a bug. Pick the most extreme shape
  *in the region* instead. Related trap: the per-volume debug dump prints one
  block per avatar, and reading numbers out of it without filtering by agent id
  attributes a stranger's chunky shape to your own (this cost several hours and
  several aditi logins).
- **`SL_VIEWER_TPOSE` is a poor tool for checking whether a mesh follows the
  skeleton**: the bind pose *is* the T-pose, so a rigged mesh that has stopped
  following its joints entirely looks identical to a correctly posed one.
- A rig binding a collision volume means nothing if it puts no *weight* on it.
  The bind-time diagnostic reports both
  (`binds N collision volume(s) … X% of its skin weight rides them`); the aditi
  agent's mesh body binds 26 volumes with 86% of its weight on them, i.e.
  genuinely fitted, while much of its clothing is classic-rigged (0%) and cannot
  follow the shape at all.
- **Still missing: the collision volumes' scale *inheritance*.**
  `LLPolySkeletalDistortion::setInfo` also scales a deformed bone's
  collision-volume **children** by `cv_rest_scale ⊙ bone_scale_deformation`
  (`inheritScale()` is true only for `LLAvatarJointCollisionVolume`), so a body
  / torso / leg-thickness slider fattens the volumes as well as the bones. We
  deliberately skipped that in P13.4 because the volumes were not rendered —
  which P17.2 made false. See the `viewer-p34-4` task.
