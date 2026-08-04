---
id: viewer-sun-disc-grey-aditi-hdr-scale
title: Sun disc renders grey on aditi (EEP sky needs sky_hdr_scale)
topic: viewer
status: done
origin: viewer-clouds-sun-occlusion-horizon-contact investigation (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact]
---

Context: [context/viewer.md](../context/viewer.md).

On **aditi** the sun **disc** renders as a flat grey circle, darker than the
surrounding bright sky (a user report during the sky-colour / bloom work),
whereas Firestorm's sun is a bright, bloomed orb. The disc's own shader
(`sun_disc.wgsl`) is a faithful port of `sunDiscF.glsl` and the draw order is
correct — the disc is simply not bright enough to blow out.

**Suspected cause:** the reference scales all WL-sky pixels (including the disc)
by `sky_hdr_scale` before tone-mapping. For **legacy / classic-mode** skies that
factor is `1.0` (the shipped default, `RenderSkyAutoAdjustLegacy = false`),
which is what the 2026-08-03 sky-colour fix assumes. But an **EEP** sky with a
non-zero `reflection_probe_ambiance` sets `sky_hdr_scale = sqrt(ambiance) * 2`
(> 1), which pushes the disc above 1.0 so it blows out (and, with bloom,
haloes). We do **not** decode `reflection_probe_ambiance` (it is not on
`SkySettings`), so we always use `1.0`, leaving the disc capped ~grey on aditi
EEP skies.

**Work:**

- Decode the sky's `reflection_probe_ambiance` into `SkySettings` (sl-proto).
- Compute `sky_hdr_scale` per the reference (`llsettingsvo.cpp`: probe-ambiance
  → `sqrt(g)*2`; legacy classic-mode → `1.0`; auto-adjust →
  `RenderSkyAutoAdjustHDRScale`) and apply it in the sky / cloud / **sun-disc**
  shaders (a new uniform, alongside the `SL_VIEWER_SKY_LINEARIZE` knob).
- Verify on **aditi** (the disc does not render on local OpenSim — its texture
  404s there), with a Firestorm side-by-side.

Not reproducible on the local grid; needs an aditi login.

**Progress (2026-08-03) — `sky_hdr_scale` done; disc grey is broader; harness
built:** `sky_hdr_scale` is implemented faithfully (`reflection_probe_ambiance`
decoded in sl-proto; `sqrt(gamma)*2` for EEP / `1.0` legacy, per
`llsettingsvo.cpp`; applied to the sky / cloud / sun-disc shaders as a new
uniform + an `SL_VIEWER_SKY_HDR_SCALE` A/B override). But live-testing showed
the grey disc is **broader than EEP**: it also reads grey on **legacy** skies
(where `sky_hdr_scale = 1.0`), because the sun texture is a pure-white **LDR**
sprite (linear ~1.0) that alpha-blends *over* the brighter near-sun **HDR** haze
and so reads as a dim hole (on OpenSim the disc texture 404s, so the bug only
shows where the disc actually loads). An additive/max-blend disc hack fixed noon
but broke sunset (over-bloom), so it was reverted to the faithful
`srgb_to_linear(texture) * sky_hdr_scale` alpha-blend — the fix has to be in the
formulas, not the blend.

To fix it against **byte-identical input** to Firestorm, built a **World >
Environment comparison harness**: three groups x four times (Day Cycle = the
region's own EEP frozen per time; Legacy = ported `A-*`; Modern = the real
`KNOWN_SKY_*` EEP library skies Firestorm loads) + Use Shared. Needed a new
`AT_SETTINGS` fetch/decode path: sl-proto `EnvironmentAsset` +
`environment_asset_from_bytes` (LLSD-format-detecting), and the viewer
`EnvironmentAssetManager` (fetch by UUID over `ViewerAsset`, decode, cache;
mirrors `AnimationManager`). Live-validated on aditi: Modern fetch/decode works
(sunset asset swaps in at the horizon, matching Legacy); Day Cycle is faithful
(region cycle at 0.25/0.75 puts the sun mid-altitude, not the horizon — the
region's authored cycle, not a bug).

**Progress (2026-08-04) — dynamic exposure DONE; disc is NOT a glow bug;
faithful glow port started:**

- **Dynamic exposure ported** (`exposure.rs` + `exposure.wgsl`, unit-tested,
  clippy-clean): a fullscreen pass grid-samples the composited scene's average
  luminance over the reference central crop and evaluates the `exposureF` curve
  `s = mix(exp_max, exp_min, pow(clamp(L/coeff,0,1),2))` into a 1×1 exposure map
  the tone mapper samples (`final_exposure = RenderExposure · s`). The
  `exp_min/exp_max` range is the `generateExposure` `[1/hdr_scale, hdr_scale]`
  (EEP) / `(1,1)` (legacy) counterweight, computed from the active sky's
  `reflection_probe_ambiance`/`gamma` and published by `drive_sky`. No-fade path
  (`gExposureProgramNoFade`); history smoothing not ported. Env A/B:
  `SL_VIEWER_DISABLE_DYNAMIC_EXPOSURE`, `SL_VIEWER_EXPOSURE_COEFFICIENT`. Needs
  an aditi run to see its effect (EEP-only; a legacy sky is a no-op).
  **NB it does not, on its own, fix the grey disc** — a uniform pre-tone-map
  multiply does not change the disc-vs-sky ratio.
- **`sky.wgsl` near-sun haze audited: it is a faithful port** of `skyV.glsl` /
  `skyF.glsl` (the `haze_glow`, the `color*2`, `clamp(0,5)`, `srgb_to_linear ·
  sky_hdr_scale`), line-for-line — no discrepancy. So the grey disc is not a
  haze-formula bug.
- **The reference sun disc does NOT bloom via the glow pass.** Traced: the disc
  is drawn deferred (`sunDiscF` writes the G-buffer with `SKIP_ATMOS`);
  `softenLightF`'s SKIP_ATMOS branch emits `srgb_to_linear(baseColor)·hdr_scale`
  and sets `frag_color.a = 0.0`, zeroing the disc's glow-mask alpha; the disc is
  `POOL_WL_SKY` not `POOL_GLOW`; and `generateGlow` runs the luminance extract
  at `minLuminance = 9999` (off) — so SL glow is **alpha-mask-driven**, not
  luminance-driven, and neither the sky nor the disc feeds it. The disc's
  on-screen brightness is purely `srgb_to_linear(disc)·sky_hdr_scale` (~1.0 on a
  legacy sky). **So the grey disc is a disc-vs-near-sun-haze brightness /
  tone-map interaction, not a glow problem**, and its root cause (sky-param
  decode vs tone-map vs disc-texture) needs an
  **aditi pixel comparison against Firestorm** to isolate — still OPEN.

**Progress (2026-08-04) — ROOT CAUSE FOUND + faithful fix (the 8-bit
deferred G-buffer clamp):** a live aditi pass (all 12 World > Environment
settings) made the diagnostic sharp: the **moon renders perfectly** (a real
256² RGBA lunar disc on the black night sky) while the **sun is a flat grey
circle** in every daytime setting — same shader, same code path, the only
difference being *what sits behind the disc*. A decoded-pixel dump
(`SL_VIEWER_LOG_SKY_HDR` now also logs the disc texture) confirmed the sun
texture is a correct, pure-white **opaque** disc (`512²`, RGBA,
centre `[255,255,255,255]`), so `srgb_to_linear(1.0) · hdr = 1.0` linear.

The bug is on the **sky** side, and it is an architecture-faithfulness miss: the
reference draws `skyF` / `cloudsF` / `sunDiscF` into `deferredScreen`, an
**8-bit `GL_RGBA` (UNORM, [0,1]) G-buffer** (`pipeline.cpp`
`deferredScreen.allocate(.., GL_RGBA, ..)`; `RenderEnableEmissiveBuffer`
defaults false so the sky uses `frag_data[0]`, and even the optional emissive
attachment is `GL_RGB` — all 8-bit). That buffer
**clamps the sky's sRGB colour to [0,1] before** `softenLight` reads it back and
applies `srgb_to_linear · sky_hdr_scale`. So the reference near-sun haze maxes
at `1.0 · hdr` — exactly the white disc's ceiling — and the two blend into one
bright orb. The disc is drawn `BT_ALPHA` (`llgl.cpp`
`LLGLSPipelineBlendSkyBox`), not additive, so it genuinely replaces the
(equally-bright) haze — no hole.

Our forward sky has no such buffer and linearised `clamp(haze, 0, 5)` in float,
so the near-sun haze reached `srgb_to_linear(5) ≈ 42` and dwarfed the 1.0 disc
→ the grey hole. (The moon escaped it because a black night sky has nothing
bright behind it.) **Fix:** clamp the sky (`sky.wgsl`) and cloud (`clouds.wgsl`)
sRGB colour to `[0,1]` immediately before `srgb_to_linear`, emulating the 8-bit
G-buffer. This caps haze / lit clouds at `1.0 · sky_hdr_scale` (the disc's
ceiling) for **every** legacy / EEP sky — a faithful, general fix, not a
per-case tweak. The sun disc itself is already faithful (`≤ 1 · hdr`), so it is
unchanged; stars are additive and already `≤ 1`.

**Verified (2026-08-04):** live aditi pass across all nine daytime World >
Environment settings (Day-Cycle / Legacy / Modern × sunrise / noon / sunset) —
the grey disc is gone in every one; the sun reads as a bright orb, and the
clouds are not over-bright. The moon (night) was already correct. Resolved.

- **Faithful glow port** (user-approved) — replace Bevy's mip-chain `Bloom` (a
  tuned approximation whose strength never generalises) with SL's alpha-mask
  separable-Gaussian glow. Staged so every intermediate builds and runs:
  - **Step 1 DONE** — the post-process core (`glow.rs` +
    `glow_extract`/`glow_blur`/`glow_combine.wgsl`): extract `rgb·alpha` → a
    512² ping-pong of `RenderGlowIterations·2 = 4` separable Gaussian passes
    (the `[.25,.5,.8,1,1,.8,.5,.25]` kernel at `delta·[-3.5..3.5]`,
    `delta = RenderGlowWidth/512`, `× RenderGlowStrength 0.325`) → additive
    `scene + glow`. Ordered **after** the tone mapper (`SlTonemapPass`) — the
    reference runs glow in `renderFinalize` after `tonemap`, over the
    display-space frame × the alpha mask (which survives fog/exposure/tonemap,
    each passing alpha through). **Disabled by default**
    (`SL_VIEWER_ENABLE_GLOW=1`), coexisting with the Bevy `Bloom` so the scene
    is unchanged until the mask is fed. Env knobs `SL_VIEWER_GLOW_STRENGTH` /
    `_WIDTH`.
  - **Step 2a DONE** — feed the alpha glow mask on the main paths: a `glow`
    scalar on `SlFaceParams`, carried into the extension at build
    (`textures::face_material`); `face_material.wgsl` writes it to
    `out.color.a`. `sky.wgsl` and `terrain.wgsl` (both opaque) now write alpha
    `0`. Inert while glow is disabled. The P27.4 glow→emissive stays for now so
    the current Bevy `Bloom` is unchanged; Step 3 removes it.
  - **Step 2b DONE** — make the mask correct for **all** surfaces so it is right
    when enabled:
    - The glow write is now **gated in `face_material.wgsl` on the alpha mode**
      (`pbr_input.material.flags` → `OPAQUE`/`MASK` write the mask, others leave
      alpha), so `glow` defaults to `0` and **every** opaque face feeds mask `0`
      without each CPU build site (`avatars`, `edit_material`, …) having to set
      it — and a blend face keeps its coverage automatically. This replaced the
      2a `< 0` sentinel.
    - Alpha-blended materials preserve the mask: a shared
      `sl_client_bevy::preserve_glow_mask_alpha` (gated `bevy_pbr`) overrides
      the **alpha** blend component to `(Zero, One)` (colour/coverage
      untouched), called from `sun_disc` / `water` / `stars` / `clouds`
      (sl-client-bevy) and `parcel_borders` (viewer) `specialize`.
      **Particles deferred** — their custom `particle_render` pipeline sets
      per-blend-mode targets and glowing particles are a legitimate case; left
      feeding coverage (a documented limitation) for a focused pass.
    - Validated: `cargo check` + `clippy --all-targets` clean on both crates + a
      render-readback face test passes (the alpha-mode-gated shader renders).
  - **Step 3 DONE (code) — glow enabled by default; Bevy `Bloom` removed.** The
    glow pass is on by default (`SlGlow::default().enabled`, off via
    `SL_VIEWER_DISABLE_GLOW`); the Bevy `Bloom` component + `bloom.rs` are gone;
    the P27.4 glow→emissive hack is removed from `bump::apply_surface_flags` (a
    glowing face now feeds the alpha mask, not `emissive`); and the `RenderGlow`
    / `RenderGlowStrength` / `RenderGlowIterations` / `RenderGlowWidth` settings
    are registered on the glow module with a `refresh_glow` system (env
    overrides win). The `.after(bevy::post_process::bloom::bloom)` orderings on
    exposure / fog / tonemap are left as-is — Bevy's bloom *system* still exists
    in the schedule (just does nothing without the component), so the labels
    resolve. All materials audited: the only opaque main-scene materials are the
    gated `FaceMaterial`, `sky` and `terrain` (mask 0), so nothing over-blooms.
    Validated: `cargo check` + `clippy --all-targets` clean, bump + a
    render-readback face test pass.
    - **Aditi live-verified (2026-08-04):** glow-flagged / fullbright / emissive
      builds (neon signs, emissive prims, fullbright blobs) bloom like
      Firestorm; distribution correct. Fixed one bug found live — the glow
      samplers used wgpu's `SamplerDescriptor::default()` (**Nearest**), so the
      512² buffer was point-sampled and the bloom looked blocky; switched to
      **Linear** mag/min (the reference's trilinear glow sample) and it is
      smooth. This is the in-world-fidelity goal — **not** the grey-disc fix
      (the disc doesn't feed the SL glow).
  - **Cleanups DONE (2026-08-04) — glow finished:**
    - **Per-particle glow ported** — the decoded `PSYS_PART_*_GLOW` is now
      interpolated per particle (`particles.rs`, like the colour), carried on
      the `ParticleInstance` (`@location(8)`), and the **additive** particle
      path writes it to alpha (`PARTICLE_ADDITIVE` in `particle.wgsl`) so its
      `(One, One)` alpha blend accumulates it into the glow mask — a glowing
      (fire/light) particle blooms, a plain one does not. A non-additive
      (alpha-blend) particle preserves the mask (`preserve_glow_mask_alpha`) and
      does not glow (a documented limitation — glowing particles are the
      additive kind).
    - **Transparent prim faces no longer bloom** — the fix that was missing:
      `FaceMaterial`'s blend faces had no mask-preserve, so a transparent prim
      (glass, etc.) fed its coverage to the mask. Added
      `MaterialExtension::specialize` on `SlFaceExt` calling
      `preserve_glow_mask_alpha` — a no-op for an opaque/mask face (no colour
      blend, so its `glow` mask write still lands), the `(Zero, One)` override
      for a blend face.
    - **Editor overlays no longer bloom** — the selection outline
      (`edit_selection` `HighlightAssets`, inverted-hull) and the Select-Face
      grid cursor were translucent `StandardMaterial` on the main camera;
      converted both to an inert `FaceMaterial` (bit-identical render), which
      inherits the `SlFaceExt::specialize` mask-preserve. (`gizmos` already
      render on a separate `GizmoCamera`/layer that the glow never touches; the
      probe-debug `StandardMaterial` is a test fixture.)
    - Validated: `cargo check` + `clippy --all-targets` clean; 21 particle
      tests + a render-readback face test pass. Needs an
      **edit-mode aditi eyeball** to confirm the overlays/particles look right
      (can't verify headlessly).
