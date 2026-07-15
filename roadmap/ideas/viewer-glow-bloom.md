---
id: viewer-glow-bloom
title: Full-screen glow / bloom
topic: viewer
status: ideas
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
