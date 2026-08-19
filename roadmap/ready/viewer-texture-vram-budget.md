---
id: viewer-texture-vram-budget
title: Texture VRAM budget & global discard bias
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-statistics-floater]
---

Context: [context/viewer.md](../context/viewer.md).

A global texture-memory feedback loop: track an estimate of texture VRAM in
use against a budget (auto-detected from the adapter, overridable), and
under pressure raise a **global discard bias** that the per-texture
discard-level selection (P21.1 screen-importance × discard) adds in — so a
heavy scene degrades resolution uniformly instead of thrashing or
exhausting VRAM. Under sustained headroom, lower the bias again
(hysteresis, as the reference's `sDesiredDiscardBias` does).

First step is a verification pass over the current state: P21 selects
per-texture discard levels, but confirm whether any global budget /
down-bias exists yet in `textures.rs` / `sl-asset-sched`; build on what is
there. Expose current usage + bias in the statistics floater
([[viewer-statistics-floater]]) and a budget setting.

Reference (Firestorm, read-only): `llviewertexture`
(`sDesiredDiscardBias`, `RenderMaxVRAMBudget`), `lltexturefetch`.

Builds on: P21 texture discard selection and the texture cache.

## Parity-audit addendum (2026-08-19)

The parity audit adds three Develop ▸ Rendering texture-budget
toggles: **Disable Textures** (drop textures to lowest discard for
budget triage), **Full Res Textures** (force full-resolution, the
opposite override), and **Reduce Draw Distance when VRAM is full**
(automatic draw-distance stepping under VRAM pressure) — dev-facing
knobs over the same budget machinery this task builds.

Beyond the pressure-driven budget/bias already in scope, the reference
graphics tab adds three related behaviours to fold in:

- `FSDrawDistanceVRAMOptimization`: under sustained VRAM pressure,
  temporarily reduce the draw distance as a second relief valve (and
  restore it when pressure clears), not just texture discard.
- A *user-set* ceiling independent of pressure:
  `RenderMaxTextureResolution` (cap the resolution any fetched texture
  decodes to) and `TextureDiscardLevel` (a fixed global discard floor).
  The task body currently has only the pressure-driven bias; these are
  explicit user knobs on top of it.
- `TextureDiscardBackgroundedTime` / `TextureDiscardMinimizedTime`: shed
  texture memory after the window has been backgrounded or minimized for
  N seconds, re-sharpening on refocus.
