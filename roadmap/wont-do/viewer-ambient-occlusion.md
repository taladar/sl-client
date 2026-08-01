---
id: viewer-ambient-occlusion
title: Screen-space ambient occlusion
topic: viewer
status: wont-do
origin: user request (2026-07)
refs: [viewer-screenshot-wait-for-quiescence, viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

**Won't do: implemented and evaluated live, and no iteration ever looked
better or more realistic than the scene without it.** The whole pipeline was
built and driven against a hut interior on the local grid (built-in Bevy VBAO
first, then a custom hemisphere-sampling kernel), and the user — reviewing every
variant on their own build — judged that none improved the look. The changes
were reverted; the working tree is back at the pre-task commit.

Why screen-space AO could not deliver here (the durable reason not to retry it
standalone):

- **It modulates a flat, dim indirect term.** AO only attenuates *indirect*
  light. Our only indirect term indoors is the reflection probe's diffuse IBL
  (P33), which is already fairly flat and modest — multiplying a flat, dim
  ambient by an occlusion factor is a small absolute change, so there is little
  contrast for AO to carve into. The reference viewer's AO reads because it sits
  inside a whole deferred lighting model with richer indirect light.
- **Screen-space ⇒ view-dependent and one-sided.** Horizon methods (Bevy's
  GTAO/VBAO) resolve an occluder well from a *grazing* surface but poorly from a
  *head-on* one, so a 90° interior corner darkened one-sidedly — the artifact
  the user found most unnatural. A custom hemisphere-sampling kernel (sampling
  the normal-oriented hemisphere against the depth buffer) did make the corner
  darkening symmetric, but it was still only a soft, subtle band and did not
  read as an improvement. All screen-space AO can only see the current depth
  buffer, so some view dependence is inherent (the reference's SSAO included).
- **SL's scale fights it.** SL architecture is larger than RL and is viewed from
  a far third-person camera, so the local AO darkening bands are small on
  screen; a radius large enough to matter across a big room blobs a small room
  and costs performance.

The genuinely better-looking version of this ("moody, grounded interiors")
comes from **baked AO** or **global illumination**, not SSAO — a much larger,
different project, and awkward for arbitrary dynamic user builds. If interior
lighting fidelity is revisited, do it there, not as a standalone screen-space
pass. Related lighting work: reflection probes (P33) and the P33.3 tonemapper.

What a retry would have to change (all reverted here): SSAO forces `Msaa::Off`
(a Bevy requirement) + a depth/normal prepass, so the whole viewer had to move
to `Fxaa` and every window camera (world / gizmo / HUD / name-tag) to
`Msaa::Off`; the custom terrain / water / sky materials had to opt out of the
prepass (or, for terrain, opt *in* so ground-built corners get AO); the
underwater-fog pass had to bind single-sampled depth; and the custom tonemap had
to be ordered before FXAA. That blast radius, for no visible gain, is itself a
reason to keep this shelved rather than carry the pipeline churn.
