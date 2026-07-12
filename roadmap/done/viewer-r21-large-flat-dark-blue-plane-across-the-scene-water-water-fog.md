---
id: viewer-r21
title: Large flat dark-blue plane across the scene (water / water fog?)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R21. Large flat dark-blue plane across the scene (water / water fog?)**
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
