---
id: viewer-ambient-occlusion
title: Screen-space ambient occlusion
topic: viewer
status: ideas
origin: user request (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The viewer has **no ambient occlusion at all** — no SSAO, no GTAO, no HBAO, no
GI. The only "occlusion" in the codebase is the PBR *occlusion map*, which is
already honoured: SL packs it into the ORM texture, so `materials.rs` binds one
image to both `metallic_roughness_texture` and `occlusion_texture` (Bevy reads
occlusion from its red channel). That is texture-space, per-material, and does
nothing for contact shadows between separate objects — which is what makes an
un-occluded SL scene look flat and floaty.

Bevy 0.19 ships `ScreenSpaceAmbientOcclusion` in its default features and we
simply never enable it, so the first decision is whether the built-in pass is
good enough or whether the reference viewer's look needs a custom one
(Firestorm's SSAO runs in its deferred pipeline, with its own radius / falloff /
blur controls exposed as debug settings).

The interactions matter more than the effect itself, and are where this task
will actually be spent:

- **Reflection probes (P33).** `probes.rs` deliberately zeroes the flat sky
  ambient (`suppress_global_ambient`, `probe_ambient_scale` defaulting to 0) so
  the probe's image-based irradiance is the *only* ambient term. AO must
  attenuate that irradiance, not a `GlobalAmbientLight` that no longer
  contributes — check where Bevy applies the SSAO term and whether it reaches
  the environment-map light at all.
- **The custom tonemapper (P33.3).** The camera runs `Tonemapping::None` and we
  post-process ourselves (`tonemap.rs`). Confirm the AO pass lands *before*
  that, in linear space, and does not fight the calibrated curve.
- **Shadows (P24).** Cascade shadows already darken contact regions; AO must
  complement rather than double-darken them.

Include quality / performance tiers and a preferences toggle
([[viewer-preferences-ui]]), and verify with a headless A/B: absolute camera
pose plus the screenshot harness, with a per-effect disable env toggle in the
style of the existing `SL_VIEWER_DISABLE_*` switches. Reproducible captures want
[[viewer-screenshot-wait-for-quiescence]] first, otherwise the two frames differ
for reasons that have nothing to do with AO.

Reference (Firestorm, read-only): the deferred `*ssao*` shaders,
`RenderDeferredSSAO` and friends, `llpipeline`.

Builds on: P24 shadows, P33 reflection probes, and the P33.3 tonemapper.
