---
id: viewer-perf-wgpu-texture-init-fastpath
title: wgpu fast-path for fully-initialized textures (render-encode CPU)
topic: viewer
status: done
origin: submit/render-encode profiling (aditi, 2026-08-16), spun out of
  viewer-perf-per-object-face-merge-entity-count Step 0
refs:
  - viewer-perf-per-object-face-merge-entity-count
  - viewer-perf-render-app-bound-frame
---

Context: [context/viewer.md](../context/viewer.md).

## What & why

The viewer frame is render-CPU-bound (render leg ~46.8 ms/frame gating vs main
~25 ms, GPU ~5.5 ms idle — see
[[viewer-perf-per-object-face-merge-entity-count]] Step 0). A `perf` sample of a
dense aditi scene pinned a large slice of the render thread to **wgpu's
per-frame texture memory-init tracking**: `register_init_action` (1.87 % self) →
`TextureInitTracker::check_action` (0.30 %) walks every mip level of every
texture on every use — once per texture bind per draw per view — plus the
`TextureSurfaceDiscard` vec churn (0.59 %). This is pure overhead for our
textures, which are uploaded once and never de-initialized: a fully-initialized
tracker can never yield an init action, yet the per-mip walk + allocation ran
regardless.

## Fix

A one-change fork of **wgpu 29.0.4** (`github.com/taladar/wgpu`, branch
`sl-client-texture-init-fastpath`, rev `72126fe`): `register_init_action`
short-circuits when the texture is fully initialized (`InitTracker`'s
uninitialized-range list empty — O(1)) **and** no discards are outstanding,
skipping `check_action` and the discard scan. The result is byte-identical to
the slow path (which would extend nothing and retain every discard); only the
redundant bookkeeping is skipped. Added `is_fully_initialized()` to
`InitTracker` and `TextureInitTracker`. **Not submitted upstream.**

Wired like the bevy fork: all nine wgpu-monorepo crates (`wgpu`, `wgpu-core`,
`wgpu-hal`, `wgpu-types`, `wgpu-naga-bridge`, `naga`, the three
`wgpu-core-deps-*`) pinned to the fork rev via `[patch.crates-io]` (siblings
depend by path, so patching one drags the rest — leaving any on crates.io splits
the graph); `naga_oil` stays on crates.io (uses the forked `naga` transitively).
`deny.toml`'s `allow-git` gained the fork URL. To bump wgpu: re-cut the branch
off the new tag, re-apply the three-file diff, update the rev on every
`[patch]` line. The parallel bevy fork (same pinning pattern) is documented in
the workspace `Cargo.toml` `[patch]` comments.

## Measured (aditi, comparable dense scenes, steady-state)

- `perf` (variance-free): `check_action` eliminated from the profile;
  `register_init_action` 1.87 % → 0.96 %; `TextureSurfaceDiscard` churn
  0.59 % → 0.29 %. The residual is the guard itself (an unavoidable RwLock read
  plus the `is_fully_initialized` mip-loop).
- Tracy: render-encode CPU dropped where `check_action` lived (transparent-pass
  encode and submit both fell); frame p50 improved, though the exact delta is
  partly run-to-run scene variance (the main-thread schedule, which a wgpu
  change cannot touch, also moved between runs).

Honest read: a real, behaviour-identical render-encode CPU reduction — roughly
half the texture-init-tracking cost — that scales with texture-bind volume /
scene density, for the cost of maintaining a wgpu fork.

## Follow-ups (not done)

- The residual `register_init_action` cost could shrink further with an O(1)
  cached "fully initialized" flag on `TextureInitTracker` (updated on
  drain/discard) instead of the per-mip `is_empty` loop — more invasive,
  deferred as diminishing returns.
