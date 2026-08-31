---
id: viewer-render-fixtures-vendored-assets
title: Render-tier avatar fixtures default to the vendored character assets
topic: viewer
status: done
origin: user request (2026-08-31) — vendoring the assets raised the question
points: 2
refs: [viewer-render-context-matrix, viewer-render-animation-coverage]
---

Context: [context/testing.md](../context/testing.md).

Since 2026-08-31 the Linden character assets are vendored at
`viewer-assets/character/` and are the viewer's default; the render
tier deliberately still treats them as opt-in: `render_scene.rs` reads
`SL_VIEWER_ASSETS` (`VIEWER_ASSETS_ENV`) and falls back to the mini
4-vertex fixture, and `render_test.rs`'s morphed-body comparison
returns early when the env var is unset. That kept the R0 sweep's
avatar silhouettes stable through the vendoring — a real body has a
different silhouette, coverage and bounds than the mini fixture.

The switch: default both to the vendored directory (resolved via
`CARGO_MANIFEST_DIR`, the same way `world_test.rs` and the viewer
binary do), so the avatar render scenes exercise the real skeleton,
LAD morphs and base meshes on every run — the correctness the assets
were vendored for — and CI needs no environment.

- Re-measure and re-verify the R0 canonical sweep's avatar subjects
  (`measured_bounds`, coverage, corner uniformity) against the real
  body; adjust `SubjectSignature` bounds/points where the mini fixture
  had calibrated them.
- The morphed-body comparison in `render_test.rs` runs unconditionally
  once assets are always present — its early-return gate goes away.
- Keep an explicit escape hatch for A/B work (e.g.
  `SL_VIEWER_ASSETS=mini` or a test-only override) so the mini fixture
  remains reachable when a regression needs to be bisected between
  "asset content" and "render path".
- `viewer-render-animation-coverage`'s "all three additionally need
  `SL_VIEWER_ASSETS`" pre-condition becomes moot — the skeleton is
  always available.

Acceptance: the full GPU suite passes with no `SL_VIEWER_ASSETS` in the
environment, and the avatar scenes demonstrably render the real body
(the morphed-vs-base comparison bites).

Done (2026-08-31). `render_scene.rs` gained `vendored_character_dir`
(off `CARGO_MANIFEST_DIR`, the idiom the viewer binary and
`world_test.rs` already use) and `pub fn avatar_assets_dir`: an explicit
`SL_VIEWER_ASSETS` path wins, `SL_VIEWER_ASSETS=mini` is the bisecting
escape hatch back to the 4-vertex fixture, and unset means the vendored
directory. Both avatar spawners and `render_test.rs`'s morphed-vs-base
comparison go through it, so that comparison now bites on every run
rather than returning early.

Re-measured against the real body, R0's one adjustment was
`avatar-base-part`: it paints **0.125** of its own silhouette, under the
0.15 floor, and honestly so — the `.llm` meshes are authored in the
T-pose, so a 1.7 m arm span boxes a thin cross of geometry and most of
the disc is air. It joins `SPARSE` at 0.08 (still catching "drew
nothing" and "lost half the body") with that reason. `avatar-morphed-body`
needed nothing at 0.226, and only because `shaped_appearance` flares its
skirt from 0.3 m to 1.3 m wide — the same T-pose underneath. No other
subject moved; the whole 151-test viewer suite is green with no
environment set, and green again under `SL_VIEWER_ASSETS=mini`.
