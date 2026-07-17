---
id: viewer-render-readback-tier
title: Render readback tier — assert on the pixels, headlessly
topic: viewer
status: ready
origin: the viewer-render-test-harness work (2026-07); the second half of that task's "automatic assertions", deferred once the geometry tier proved out
blocked_by: [viewer-render-test-harness]
refs: [viewer-render-test-harness, viewer-render-scene-coverage, viewer-screenshot-wait-for-quiescence]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-render-test-harness]] built the geometry tier: real converters, real
scenes, in `cargo test`, no GPU. It answers "is this geometry valid" and it does
not — and cannot — answer **"did the right pixels light up"**.

That gap is not academic. A whole class of the registry is *about* pixels and is
currently registered with nothing checking it:

- `projector-light-on-wall` — the light is either falling on the wall or it is
  not, and no vertex knows.
- `point-light-between-prims` — falloff and the local-light budget.
- `metallic-sphere-among-prims` — the scene exists so a reflection has something
  to reflect; there is no reflection check.
- Sky, water, clouds, stars, tonemap, underwater fog — custom shaders whose
  output is their entire behaviour.

## Status: the mechanism exists; the coverage does not

**Built** (`src/render_readback.rs`): a headless capture (`capture`) that
renders a registered scene to a texture, reads the frame back, and projects
world points onto it through the camera that drew it — plus the first real
check, `the_mirror_reflects_each_neighbour_on_its_own_side`, which catches
[[viewer-r22i]] on its own. What remains is **every other pixel question**
below.

Three things it cost to learn, so the next check does not re-learn them:

- **Restrict to the object's pixels.** The first version sampled the whole
  frame, where the coloured prims are *directly visible* as well as reflected —
  so the centroids measured the prims, not the mirror, and it passed with R22i
  reintroduced. Project the object's silhouette with `Camera::world_to_viewport`
  and sample only inside it; do not guess a disc from the field of view.
- **Pick the axis the bug actually moves.** R22i rotates about **X**, and a
  rotation about X does not move the X axis — the "obvious" red-left/green-right
  assertion is green while the world turns underneath it. Only the below/behind
  pair discriminates.
- **The probes need ~400 frames.** `crate::probes` captures one cube face per
  frame in bursts and Bevy then filters the cube; at 90 frames a mirror reads
  pure **black** (a metallic surface takes all its colour from the environment
  map) and a check fails for entirely the wrong reason. Hence ~20 s per capture
  — the one genuinely expensive check in the suite.
- **Detect "no GPU" by outcome.** `app.get_sub_app(RenderApp)` reports `false`
  on a machine that renders perfectly well; it would have skipped this tier
  everywhere, silently. Ask whether a frame came back.

## What it is

Render each scene to a texture headlessly and read it back. Bevy has the pieces:
`GpuReadbackPlugin` ships inside `RenderPlugin` (`bevy_render-0.19`'s plugin
group), a `Camera` targets `RenderTarget::Image`, and
`Readback::texture(handle)` plus a `ReadbackComplete` observer returns the
bytes. No fork, no upstream PR — this was checked while the geometry tier was
built.

## What to assert, and what not to

**Not golden images.** The task's own warning, and it is right: pixel-exact
comparison across drivers turns the suite into a driver-version detector, and a
suite that fails on a Mesa upgrade is one that gets disabled.

Assert the things that are *decidable*:

- **Coverage** — the object covers a plausible fraction of frame. Catches "it
  rendered nothing" and "it filled the screen", which are the two failures that
  actually happen.
- **Nothing is NaN, fully black, or fully transparent** when it should not be.
- **Silhouette symmetry** where the geometry declares `SymmetricAbout` — the
  declared tier, extended into pixels.
- **A/B against a toggle** — the shape that localised R21: render with the
  effect on and off and assert they *differ* (and, for a projector, that the lit
  region is brighter than the unlit one). This is much more robust than any
  absolute value and needs no reference image.

## Fold the log check in — this is where it pays

The harness's `LogCapture` universal (no `WARN`/`ERROR` while a scene runs) is
in the geometry tier today, where it can only see viewer-side logs.
**R26 was logged by Bevy's mesh allocator, which lives in the render app** — so
the check that would have caught R26 only really bites once there is a renderer.
Wiring `capture_logs` into this tier is a few lines and closes that.

## Costs to accept up front

- **Needs a real adapter.** Keep it strictly separate from the geometry tier, as
  it is now: the cheap tier must never depend on the expensive one, or a machine
  without a GPU loses the tier holding most of the value. A missing adapter
  should skip loudly, not fail — and not silently.
- **Quiescence.** [[viewer-screenshot-wait-for-quiescence]] is a hard dependency
  in spirit: an assertion taken before the scene settles is a flake. Without a
  grid this is much easier — everything is local, and the harness already drives
  time deterministically (`TimeUpdateStrategy::ManualDuration`), so "settled" is
  a known number of frames rather than a guess.
