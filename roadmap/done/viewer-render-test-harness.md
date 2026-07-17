---
id: viewer-render-test-harness
title: 3D render test harness — render one object without a grid, assert automatically, catch regressions
topic: viewer
status: done
origin: asked for alongside viewer-ui-test-harness (2026-07), as the 3D counterpart of the same mechanism
refs: [viewer-ui-test-harness, viewer-screenshot-wait-for-quiescence, viewer-ui-radial-menu, viewer-render-scene-coverage, viewer-render-readback-tier, viewer-render-baselines, viewer-render-closedness-check, viewer-render-cpu-skinning-crosscheck]
---

Context: [context/viewer.md](../context/viewer.md).

The 3D counterpart of [[viewer-ui-test-harness]]. Every argument that task makes
applies here, and applies harder: rendering bugs are found by a human logging
into OpenSim, rezzing an object, flying the camera to it and squinting — and the
`R*` bug list in `bugs/` is what that process misses.

The `viewer-r22-avatar-render-wip` memory records the cost directly: R22 was
split into seven sub-items, three of which were "committed but do NOT visibly
fix" — because the only way to tell was another login, another bake, another
screenshot. That is the loop this task ends.

Three halves, matching the UI harness, and the same reasoning behind each.

## 1. Render one object without a grid

The precondition for everything else. Today, seeing a prim / mesh / sculpt /
avatar means: start OpenSim, log in, provision the object by OAR import (the
UUID is regenerated, so look it up in `bin/OpenSim.db` first), fly the camera
there. That is minutes per iteration, needs a human, and half of it is grid
administration rather than rendering.

Instead: a binary that takes an **asset** — a decoded prim, an `.llm`, a mesh
blob, a sculpt map, an `avatar_lad.xml` body, a `.anim` — puts it in an empty
scene with a known camera and known lighting, and renders it. No login, no
region, no OAR, no UUID lookup. The gallery of [[viewer-ui-test-harness]], for
geometry.

The corresponding constraint is the same one, and it must be stated up front
because retrofitting it is the expensive part: **an object must be
constructible without a session**. A mesh that can only be spawned by a
`Session` handing it an `ObjectUpdate` is a mesh that can never be tested. The
decode path (bytes → geometry) has to be separable from the transport path
(grid → bytes), with fixture assets standing in for the grid. Some of this
already holds — `sl-mesh`, `sl-sculpt`, `sl-terrain` are sans-I/O — so the work
is mostly in `sl-client-bevy-viewer`'s spawn systems, which currently reach for
live `ObjectState`.

## 2. Automatic assertions

The hard half, and the reason this is not just "a screenshot tool". What can a
machine actually check about a rendered object? More than it looks like, and
none of it needs a human:

- **Geometry invariants, no rendering at all.** Vertex count matches the LOD's
  header; no NaN / infinite positions; normals are unit length; UVs inside
  `[0,1]` unless the face's texgen says otherwise; the bounding box matches the
  prim's declared scale; a rigged mesh's skin weights sum to 1 (**this one has
  already shipped a real bug** — `sl-client-rigged-mesh-skinning` records that
  Bevy does not renormalise, which is exactly the R1 distortion); no vertex
  bound to a joint outside the render-list (the R13 armpit spike).
- **Render-target readback.** Render to a texture headlessly and assert on the
  pixels — not "does it look right" but the things that *are* decidable: the
  object covers a plausible fraction of frame; nothing is NaN / black / fully
  transparent when it should not be; an alpha-masked face is masked; the
  silhouette is symmetric where the source geometry is symmetric.
- **Cross-checks between paths.** The CPU-skinning reference in
  `sl-client-rigged-mesh-skinning` exists precisely to compare against the GPU
  result — make that a standing test rather than a debug affordance.

Expect the same discovery as the UI harness: the first honest run finds real
bugs in things nobody suspected. It did, within minutes, for text.

## 3. Regressions

The [[viewer-ui-test-harness]] work established that the tiers are *universal
invariants*, *declared intents*, and — the gap it did not fill —
**recorded baselines**. This is where baselines matter most.

Rendering has properties that must not drift even though no invariant forbids
drift: a **pie menu option's angle** ([[viewer-ui-radial-menu]]; muscle memory —
a user who has opened the same menu ten thousand times should not have to look),
an attachment point's
offset, a HUD element's screen position, the camera's default framing, an
avatar joint's rest pose. Nothing is "wrong" if these move; they are simply not
allowed to move by accident.

So: record the geometry (angles, positions, sizes) of named things into a
committed baseline, compare on every run, and require a **deliberate** baseline
update to change one. The signal is the diff in review — "this moved the Sit
option 12°" is a sentence somebody can object to, which is the whole point.

Note the conformance suite (`records/`) is a different thing and not a
substitute: it tests the *protocol* against a live grid. This tests *rendering*
against no grid at all.

## What to reuse

- [[viewer-ui-test-harness]] is the template: the registry (one list, so checks
  × objects compound), the two check tiers, the isolated binary, and the rule
  that construction is separable from wiring.
  `sl-client-bevy-viewer/src/ui_test.rs`, `ui_element.rs` and `gallery.rs` are
  the worked example; the module docs argue the design.
- The headless-app trick it proved: Bevy's render internals are `pub`, so a
  downstream crate can stand up the pipeline in a `cargo test` with a dummy
  render target. The UI half needed no fork; check whether the render half does
  before assuming it.
- The viewer's existing debug affordances already solve "put the camera in a
  known place and capture": the absolute camera-pose CLI
  (`--camera-position` / `--camera-look-at` / `--camera-spin`) and the
  `--screenshot-dir` sequence. This task removes the login from underneath them.
- [[viewer-screenshot-wait-for-quiescence]] is a hard dependency in spirit: a
  render assertion taken before the asset pipeline settles is a flake. Without a
  grid, quiescence is much easier to define — everything is local.

## Costs to accept up front

- **Headless GPU.** Render-target readback needs a real adapter; a machine
  without one (or CI) can still run the geometry-invariant tier, which is where
  most of the value is anyway. Split the two so the cheap tier never depends on
  the expensive one.
- **Fixture assets must be committed**, and some are large. Prefer the smallest
  asset that reproduces a class (one `.llm`, one sculpt map, one rigged mesh)
  over a library, and generate procedurally where possible.
- **Pixel assertions are brittle** across drivers. Assert on decidable
  properties (coverage, symmetry, absence of NaN), not on golden images — or the
  suite becomes a driver-version detector.

## Outcome (2026-07)

Built as **a registry of scenes × three tiers of check × a matrix**, following
[[viewer-ui-test-harness]]'s shape. `cargo test -p sl-client-bevy-viewer --lib
render_test` runs 16 tests in ~2 s: no window, no GPU, no login, no region, no
OAR, no UUID lookup.

- `src/render_scene.rs` — the registry (14 scenes), declared intent as
  components, procedural fixtures.
- `src/render_test.rs` — the headless app, the checks, the matrix.
- `src/render_gallery.rs` + `src/bin/sl-client-bevy-viewer-scenes.rs` — the
  gallery, the human-eyeball half.

### Two ways the task's framing was too narrow

Both surfaced in review, and both changed the core rather than the fixtures:

- **"Render one object" is the common case, not the general one.** A whole class
  of rendering is about the interaction *between* things and has no
  single-object form: a projector light is correct according to what it falls
  **on**, a reflective surface only against what it **reflects**. So the
  registry's unit is a **scene** — geometry, lights and a camera. Most scenes
  hold one object; `projector-light-on-wall` and `metallic-sphere-among-prims`
  cannot.
- **A frame is not enough either.** Particles, flexi, texture animation, avatar
  animation and body physics are not functions of one frame — a single capture
  cannot tell an emitter that works from one that emits nothing. So every scene
  carries a **`Timeline`**, driven by `TimeUpdateStrategy::ManualDuration`
  (never the wall clock, or the results depend on how fast the machine ran
  them). Declaring more than one sample *is* the declaration that something
  happens, and the harness holds the scene to it: identical geometry at the
  first and last sample fails.

### The tiers

| Tier | Question | Who decides |
| --- | --- | --- |
| Universal | is this broken? | the harness, every scene, no opt-in |
| Declared | does it match its stated intent? | the scene (`DeclaredBounds`, `SymmetricAbout`, `UvsInUnitSquare`, `SamplerMayClamp`) |
| Timeline | did anything actually happen? | the scene's `Timeline` |

Every universal check is one a past bug would have tripped: `skin_violations`
(R1's weight sum, R13's joint outside the render list), `sampler_violations`
(R22h), `LogCapture` (R26), plus NaN / non-unit normal / out-of-range index.
Each has a paired "teeth" test proving it fires on the known-bad case *and*
stays silent on the good one.

### It found a real viewer bug, which is the point

[[viewer-r22i]]: **every local reflection probe reflected the world rotated 90°
about X.** Bevy builds a probe's sampling frame from the probe entity's
*world transform*, and every object entity carries the Second Life → Bevy basis
change — so an identity `rotation` on a holder parented to a prim (which is what
`spawn_probe_holder` had) samples the world-space cube through that basis. A
neighbour below the mirror appeared to one side; one behind it appeared below.

Nothing was broken: no invariant, no log line, no crash. The probe captured, the
volume bound, the mirror was shiny, and the reflection was plausible from any
angle nobody had thought about. It needed a mirror with
**distinctly identifiable things around it** and a person asking "is the yellow
one where the yellow one should be" — which is exactly what
`metallic-sphere-among-prims` is, and it fell out within minutes of that scene
first rendering. It is now also a *pixel* check that catches it unaided.

### What the first honest run found

- **A fifth texture path that never set its sampler.** `default_particle_image`
  left Bevy's default (clamp-to-edge) in place. Latent rather than live — a
  billboard quad's UVs span exactly `[0, 1]` — but it is R22h's exact shape
  waiting for a caller whose UVs leave the unit square. Fixed.
- **The UV rule was backwards, and the run said so.** "UVs inside `[0, 1]`"
  looked like an invariant and is not: the viewer samples with `Repeat` because
  Second Life faces *tile*, so a prim UV of 1.025 is correct. The rule inverted
  into a **declared** one — `UvsInUnitSquare`, carried only by geometry that
  samples a packed atlas (the avatar's baked regions), where leaving the square
  samples a different body part rather than tiling.
- **Closedness had to be pulled.** Written, and it reported correct prims as
  broken. Two causes fixed (group per object not per face; match by position not
  index) and one not: SL tessellation emits **coincident** vertices (measured:
  closest distinct pair `0.000000 m` on a twisted torus at every LOD), which a
  position-quantized edge map cannot tell from a fold. Removed rather than
  shipped noisy — a noisy check gets ignored and then deleted. Written up as
  [[viewer-render-closedness-check]].

### Split out rather than skipped

The task's other halves are each their own file, because each is substantial and
none blocks the mechanism:

- [[viewer-render-scene-coverage]] — **the big one.** Unlike the UI, the viewer
  already renders nearly everything, and 14 scenes is a fraction of it. Terrain,
  water, sky, flexi, texture animation, bump, HUD, the real avatar: every check
  here runs against only what is registered.
- [[viewer-render-readback-tier]] — §2's "render-target readback". Its
  **mechanism landed here after all** (`src/render_readback.rs`): a headless
  capture that renders a scene to a texture, reads the frame back, and projects
  world points onto it through the camera that drew them — plus the first real
  check, which catches [[viewer-r22i]] unaided. Every *other* pixel question
  (sky, water, the projector's cone, coverage, symmetry) is still open, as is
  wiring `LogCapture` into it — which is where that check finally bites, since
  R26 was logged by the mesh allocator in the *render* app.
- [[viewer-render-baselines]] — §3, blocked on
  [[viewer-ui-baseline-regressions]] so the two share one format.
- [[viewer-render-cpu-skinning-crosscheck]] — §2's "cross-checks between paths".
- [[viewer-render-closedness-check]] — above.
