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
