---
id: viewer-glow-bloom
title: Full-screen glow / bloom
topic: viewer
status: done
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

The screen-space bloom that makes bright things bleed light — and it is a
**different feature from the per-face glow we already have**. P27.4 renders an
object's `glow` material term; this is the full-screen post-process that takes
the bright parts of the *whole frame*, blurs them and adds them back, so
emissive surfaces, the sun and bright lights halo. Every SL night scene and neon
build depends on it.

Firestorm gates it on `RenderGlow` (with `RenderGlowHDR` for the HDR path) and
tunes it with `RenderGlowResolutionPow` (downsample size — also the
"post-process quality" slider), `RenderGlowIterations` (how many Gaussian passes
→ bloom spread), `RenderGlowStrength` and `RenderGlowWidth`.

Scope: a bright-pass extraction, a downsampled separable-Gaussian blur over N
iterations, and an additive composite — fed by the HDR scene colour so it sits
correctly relative to the tone mapper (P33.3). Mind the ordering against
exposure / tonemap and, if built, [[viewer-depth-of-field]]: bloom is computed
in HDR before tone mapping.

Reference (Firestorm, read-only): the `RenderGlow*` post pass.

Builds on: the HDR scene target and the P33.3 tone-mapping stage.

**Done (2026-08-04)** — implemented as the faithful `RenderGlow` port (`glow.rs`

- `glow_extract`/`glow_blur`/`glow_combine.wgsl`), enabled by default, replacing
the Bevy `Bloom` approximation; the P27.4 glow→emissive hack is gone. Full
design, the staged 1→3 build, the live-verified aditi run, and the remaining
follow-ups (edit-mode eyeball; particle glow was also plumbed) live in
[viewer-sun-disc-grey-aditi-hdr-scale](viewer-sun-disc-grey-aditi-hdr-scale.md),
where the work happened.

**Two corrections to this task's premise, found by reading the reference:**

- SL's glow is **not** a luminance bright-pass. The one real `generateGlow` path
  runs the extract at `minLuminance = 9999` (off), so it is driven by the scene
  **alpha channel** — the per-face glow **mask** (glow-flagged / fullbright /
  emissive / additive-particle). The port carries that mask in the scene alpha
  (opaque materials write it; alpha-blended ones preserve it).
- It runs **after** tone mapping, not before: the reference does
  `tonemap → generateGlow → combineGlow` in `renderFinalize`, over the
  display-space frame, so the port orders after the tone mapper too.

Settings: `RenderGlow` / `RenderGlowStrength` / `RenderGlowIterations` /
`RenderGlowWidth` (512² buffer, `RenderGlowResolutionPow = 9`); env A/B
`SL_VIEWER_DISABLE_GLOW` / `SL_VIEWER_GLOW_STRENGTH` / `_WIDTH`.
