---
id: viewer-render-scene-coverage
title: Render-scene coverage — a scene per render path the viewer already has
topic: viewer
status: done
origin: the viewer-render-test-harness work (2026-07); the harness shipped with 14 scenes against a viewer that renders far more than 14 things
blocked_by: [viewer-render-test-harness]
refs: [viewer-render-test-harness, viewer-render-readback-tier, viewer-render-animation-coverage]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-render-test-harness]] built the mechanism and seeded a registry. The
registry is **not** the point — the checks × scenes product is, and today the
scene half is the short one.

This is the difference from [[viewer-ui-test-harness]], and it is worth stating
plainly because it inverts that task's reasoning. The UI registry seeded
*patterns* for panels that did not exist yet: there were no viewer panels to
register, so the elements were the vocabulary the future ones would be built
from. Rendering is the opposite.
**The viewer already renders nearly everything** — terrain, water, sky, prims,
sculpts, meshes, rigged meshes, avatars, trees, grass, particles, flexi, HUD,
lights, probes, post-processing — and every one of those paths is unscened,
which means every check in the harness runs against a fraction of the code it
could.

Each scene below is cheap (the mechanism exists; a scene is a `spawn` fn and a
registry line) and each one multiplies: a scene added here inherits every check
that exists now and every check added later, at every LOD, at every sample.

## The paths with no scene

Roughly in order of how much they have already cost in `R*` bugs:

- **Avatar, properly.** The one scene today (`avatar-base-part`) is a single
  decoded `.llm` on no skeleton. The R1 / R13 / R22 cluster lives in the parts
  it does *not* reach: the real skeleton (`BevySkeleton`, `base_mesh_skin`), the
  morph bake (`to_bevy_morphed_mesh`), the runtime morphs, joint overrides, the
  multi-part body with its alpha layers. Needs `SL_VIEWER_ASSETS`, so it is a
  scene that skips when the Linden `character/` dir is absent — which is a real
  cost and worth paying only for this one.
- **Terrain.** `terrain.rs`'s `build_patch_mesh` is already a pure
  `(patches, composition) -> Option<Mesh>` and already has six tests; a scene
  would fold it into the sweep and cover the region-edge / neighbour-offset
  cases those tests do not.
- **Flexi.** `simulate_flexi` is a `Time`-driven mesh rebuild — a natural
  **dynamic** scene, and the second one after particles. The chain's settle is
  exactly the "declares a timeline, must change" shape.
- **Texture animation.** `drive_texture_animations` rewrites a material's
  `uv_transform` per frame: dynamic, and a scene would catch a frame that
  animates off the end of its atlas.
- **Legacy materials / bump.** Both set a sampler (the R22h path
  `sampler_violations` guards) and both build normal maps; neither has a scene,
  so the sampler check currently only sees the one textured prim.
- **Sky, stars, sun disc, clouds, water.** Custom materials with their own
  shaders. The geometry tier says little about them; they are mostly for
  [[viewer-render-readback-tier]] — but a scene is the prerequisite for either.
- **HUD.** `hud.rs` + `hud_pick.rs` render on their own layer with an
  orthographic camera; a HUD scene would pin the attachment-point geometry the
  `sl-client-opensim-hud-test-attachment` memory currently tests by hand.
- **Tree billboard / impostor.** `tree_billboard_geometry` is the distance LOD
  the `tree` scene never reaches.
- **Animesh / control avatars**, **body physics**, **IK / locomotion** — each
  time-varying, each currently verified only by a login.

## What to watch for

- **Do not let a scene need a session.** That is the registry's one rule, and
  each path above should be checked against it *before* the scene is written;
  where a spawn path reaches for live `ObjectState`, the fix is to separate the
  decode from the transport, not to fake a session.
- **A scene that needs an env var is a scene that silently skips.** Only the
  avatar earns that. Prefer procedural fixtures (the harness's own convention)
  and, where the input is genuinely an asset, the smallest one that reproduces
  the class.
- **Expect findings.** The harness's first honest run over 14 scenes found a
  fifth texture path that never set its sampler. Fourteen more paths is more of
  that.

## Outcome (2026-07)

**14 scenes → 27.** `cargo test -p sl-client-bevy-viewer --lib render_test` runs
23 tests in ~6 s (~14 s with `SL_VIEWER_ASSETS`), still with no window, no GPU,
no login and no region. Every path this file listed has a scene except the last
bullet, which is [[viewer-render-animation-coverage]].

| Path | Scene(s) |
| --- | --- |
| Avatar, properly | `avatar-morphed-body` |
| Terrain | `terrain-patch`, `terrain-patch-seam` |
| Flexi | `flexi-streamer` |
| Texture animation | `texture-anim-flipbook` |
| Legacy materials / bump | `legacy-material-face`, `bump-face` |
| Sky, stars, sun disc, clouds | `sky-sunrise`, `sky-midday`, `sky-sunset`, `sky-midnight` |
| Water | `water-surface` |
| Tree billboard / impostor | `tree-billboard` |
| Animesh, body physics, IK / locomotion | **not done** — [[viewer-render-animation-coverage]] |
| HUD | **not done** — see that file's last note |

### The prediction the file got wrong, and it is the interesting one

"Each scene is cheap (the mechanism exists; a scene is a `spawn` fn and a
registry line)." True of most. The rest changed the **core**, and each for the
same underlying reason: a rule written while the registry held only prims was a
rule about prims, not about rendering.

- **`MAX_COORDINATE` was backwards, exactly as the UV rule was.** "Every scene
  here is a few metres across and a region is 256 m, so nothing legitimate comes
  near this" — except the viewer draws the atmosphere as a **3 km** dome, the
  clouds as a **15 km** one, the stars at 2.9 km and the ocean as a 40 km plane.
  All correct, all rejected. Inverted into a declared `WorldScaleGeometry`
  (bound
  - reason), so the rule still catches a vertex flung by a garbage matrix and
    the
  sky says how far it legitimately reaches. Second time this registry has found
  a universal that was really a declared; expect a third.
- **"Did anything happen" only ever looked at vertices.** True of particles and
  flexi and of *nothing else the viewer animates*: `texture-anim-flipbook` pages
  a flipbook by rewriting its faces' `uv_transform` and never moves a vertex.
  Measured, not assumed — reverting `digest` to the old vertex-only summary
  reports the working flipbook as frozen. It now reads the material and the
  world transform too, which is what will make the sun disc and the star field
  checkable when they are driven.
- **Three apps spawn scenes, and only one had the drivers.** The gallery kept
  its own copy of the resource/system list "exactly as `render_test` installs
  them", by comment; `render_readback` had neither the drivers nor the
  custom-material pipelines. With one dynamic scene that is latent; with four
  drivers and six `AsBindGroup` materials it is a scene that renders *nothing*
  in an app that reports it as fine. Now one `SceneRuntimePlugin` all three add.
  (`init_asset` is **not** idempotent — it inserts a fresh empty `Assets<M>` —
  so it guards against the `MaterialPlugin`s the two rendering apps also add.)
- **`SceneAssets` had to become a `SystemParam`.** Six custom material
  collections on top of four would have put the gallery's key handler past
  Bevy's system-parameter limit.
- **Three systems re-derived the sky frame, identically.** `drive_sky`,
  `drive_clouds` and `drive_sun_moon_discs` each recomputed the sun/moon
  directions, the up tests, the glow ladder and the clamped light-norm from one
  `SkySettings` — two of them with comments saying "as in `drive_sky`", which is
  a copy admitting it is one. Now `sky::resolve_sky` → `ResolvedSky`, shared by
  all three *and* by the scenes; that is what lets a scene render the real
  atmosphere rather than a hand-copied uniform block.

Smaller separations, the "decode vs transport" rule applied inward:
`legacy_materials::apply_legacy_scalars` (the pure half of
`apply_legacy_to_face`, which otherwise drags a `TextureManager` and a fetch
behind it), `water::water_normal_image`, and `pub(crate)` on the real builders
the scenes call. `sl_proto::azimuth_altitude_to_rotation` is now public: it is
the only way to *place* a sky's sun, and the reference's own convention for
doing it.

### What the run found

Two real viewer bugs, and **both needed the gallery** — the geometry tier was
green throughout, because nothing about the geometry is wrong. That is the
division of labour [[viewer-render-test-harness]] claims, doing its job.

- **[[viewer-r27]] — midnight is almost as bright as midday. Filed, then
  withdrawn: not a viewer bug.** Worth keeping in view because the wrong
  diagnosis was the obvious one and it survived a careful reading. The reference
  computes no night at all — the deferred renderer's `sunlit` is
  `(sun_up_factor == 1) ? sunlight_color : moonlight_color` attenuated by
  elevation, `moonlight_color` is literally `getSunlightColor()`, and
  `moon_brightness` is not in that path (it reaches only the sun/moon *disc*;
  `getLightDiffuse()` has no callers). Night is dark because the
  **midnight sky frame's `sunlight_color` is authored dark** — content. Our
  viewer already blends it correctly. The bright midnight was **the scenes'**:
  they moved the sun across the legacy default, which is a
  *single midday frame*, so there was no night in the data. Fixed by porting
  Linden's own `A-6AM` / `A-12PM` / `A-6PM` / `A-12AM` presets; midnight is now
  10% of midday and blue, and the stars switch themselves on from the frame's
  `star_brightness`.
- **The water's normal map was uploaded in the wrong colour space.** Fixed.
  `apply_water_textures` ran the fetched wave normals through `to_bevy_image`,
  which builds `Rgba8UnormSrgb` — and a normal map is not a colour: through the
  sRGB transfer a flat `(0.5, 0.5, 1.0)` texel decodes to about
  `(0.21, 0.21, 1.0)` and unpacks to a normal tilted well off the surface, so
  every wavelet in the sea was skewed the same way, in-world. What makes it a
  clear call rather than a judgement is that
  **four sibling paths already agree** and say so in their docs —
  `legacy_materials::build_linear_image` ("the linear colour space a normal map
  needs"), `materials::build_pbr_image`, `bump`'s generator, and `water.rs`'s
  **own** `flat_normal_image`. The sea changed colour space the moment its
  texture arrived. Fixed inline on the same reasoning the harness's first run
  fixed the fifth sampler path: unambiguous, and contained.
- **A wire doc that contradicted the code and the reference.**
  `LegacyMaterial`'s `diffuse_alpha_mode` was documented "0 blend, 1 none, 2
  emissive mask, 3 alpha mask"; the reference (`llmaterial.h`) is
  **none / blend / mask / emissive**, and `legacy_materials.rs` had it right all
  along. Found by writing a fixture that had to pick a value. Fixed.
- **The light reddening at sunrise / sunset works** — an open question, now
  measured: a 3° sun yields a diffuse of `[0.174, 0.096, 0.040]` (red ≈ 4.3×
  blue) against midday's near-white `[0.589, 0.587, 0.611]`. The same
  measurement is what pinned R27.

### The sky and water scenes were wrong before they were right

Every one of these was caught by a human looking at the gallery, and none of
them by a check. They are recorded because each is a trap the next scene can
fall into.

- **`sky` and `water` build in Bevy space, at the world root.** They are the
  viewer's *only* modules that do: everything else is built in Second Life
  metres and converted once at the scene root, but the atmosphere is not *in*
  the region, so both spawn with `Transform::default()` and build directly in
  Bevy's frame (`build_star_mesh` picks "a random direction on the upper
  hemisphere (Bevy Y up)"). Under the scene root that geometry is rotated
  **twice**: the cloud cap landed on the horizon and the star hemisphere on its
  side, both invisible — while the sky dome and the water plane rendered anyway,
  *because a sphere and a plane are symmetric about the very rotation that was
  wrong*. Two blank and two right-by-luck. Now one `bevy_space()` helper.
- **The water needed three separate fixes before it was a sea.** It read as flat
  blue fog through two rounds, and each round had a different cause — worth
  recording because one symptom covered three bugs. (1) `default_water_params`
  passes `camera_position: Vec3::ZERO` and the shader takes its fresnel from the
  view vector, so the sea was lit for a viewpoint nobody was at; `WATER_CAMERA`
  is now one constant feeding both the registry entry and the uniform. (2) The
  scene wore the viewer's **1×1 flat** normal placeholder, which by construction
  has no slope anywhere — no fresnel variation, no specular, no wave. The
  fixture now generates a tiling wavelet map (three sine waves at integer wave
  numbers so it tiles; normals analytic, since a generated field has an exact
  gradient and no reason to approximate its own). (3) The upload was sRGB —
  above, and it took building one by hand to notice.
- **Four scenes that share one camera.** Sunrise and sunset first had *mirrored*
  cameras, each looking toward its own sun — which cancels the thing being
  compared and rendered the same picture twice, with the shadow appearing to
  swing 90° (the angle between the two viewpoints) rather than the 180° the sun
  actually moved. A fixed pose is what makes the four a comparison.
- **The shadow caster floats, and the ground is 60 m.** Both follow from
  `displacement = height / tan(elevation)`. A box resting on the ground hides
  its own shadow at midday (an 80° sun puts it underneath) and at the 65° moon;
  a metre of air fixes that. But floating it is also what makes a low sun's
  shadow *long* — the 3° sun throws it ~19 m, clean off the 28 m plane the first
  version had.

### Notes for whoever adds the next one

- **The sky stack is static on purpose**, but its uniforms are *resolved*, not
  seeded (`resolve_sky`, above). The scenes stay static because the drivers
  centre their domes on the camera every frame and would fight a scene root;
  `EnvironmentState::default()` is the full legacy WindLight default and needs
  no session, so a *driven* sky scene is reachable if the parenting is solved.
- **The four times of day are Linden's own presets, ported — and they have to
  be.** `LLEnvironment::KNOWN_SKY_SUNRISE` and friends are grid asset UUIDs, but
  Firestorm ships the classic WindLight equivalents in
  `app_settings/windlight/skies/` (`A-6AM`, `A-12PM`, `A-6PM`, `A-12AM`), and
  `render_scene`'s `SkyPreset` ports their values through the reference's own
  `translateLegacySettings` rules: scalars are the `[0]` of their legacy array,
  `star_brightness` scales by 250, and the bodies come from
  `azimuth = -east_angle` / `altitude = sun_angle` with the moon
  **diametrically opposed**. Ported as constants, not read from disk — a scene
  that needs an asset is a scene that skips. The earlier version instead moved
  the sun across the legacy default and is why [[viewer-r27]] was filed against
  innocent code:
  **a sky frame is data, and the day/night difference lives entirely in it**, so
  a fixture that authors its own is not testing the viewer's sky. Read that file
  before touching this.
- **Terrain patches are placed in plain Second Life metres here**, not through
  `patch_transform` — that carries the SL→Bevy basis change because the viewer
  spawns patches at the world root, and the scene root already carries it. The
  region-offset half has its own test; the mesh had none.
- **`shaped_appearance` is deliberately not all-`128`.** That is R12's own bug:
  a midpoint vector half-applies every asymmetric morph (whose default is `0`),
  so it would make the fixture the bloated body rather than a shaped one.
- **The gallery dims a `SceneLighting::Own` scene's ambient to 12 nits**, where
  the viewer's sky sets ~100 from the frame's own ambient. It has to: `probes`'
  `suppress_global_ambient` multiplies `GlobalAmbientLight` down every frame and
  the gallery has no sky system to re-set it. So the sky scenes' shadow and
  light direction/colour are faithful and their overall fill is not.
