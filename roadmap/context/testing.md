# Context — the viewer test harness

Non-task prose for the automated visual / interaction / full-stack test
harness that replaces manual "log in and squint" verification of the Bevy
viewer. Tasks carry the `viewer` topic (viewer-side mechanism and tests) or
the `test` topic (fake-grid fixtures, determinism, conformance). Read this
before working any `viewer-render-*`, `viewer-*-interaction-*`,
`viewer-world-test-*`, `viewer-fake-grid-*` or `test-fake-grid-*` task.

## The tier rule

**A test lives in the lowest tier that can produce its failure.** This
replaces the older convention that "viewer phases verify with a live run
against the local OpenSim grid" (see `viewer.md`) and the fake-grid book
chapter's "the Bevy tier stays a smoke test". A live run is the exception,
reserved for what no tier below can reach, and is repeated through the
fake grid where content must be repeatable.

| Tier | App | Sees | Cost |
| --- | --- | --- | --- |
| G — geometry | `render_test.rs` CPU app | meshes, samplers, transforms, logs | ms |
| U — UI layout | `sl-viewer-testkit::LayoutTest` | bevy_ui layout, text measure | ms |
| I — interaction | `InteractionTest` over U; `WorldTest` fixture world | picking, drags, gizmos, pies, floaters, camera input | ms |
| R — render matrix | `render_readback.rs` GPU app + `ViewerRenderPlugins` | pixels of registered scenes under context axes | seconds per capture |
| F — full stack | viewer plugin groups + in-process `sl-fake-grid` + readback | grid sequencing: arrival, CAPS fetches, teleport, crossing, NPCs | tens of seconds per test |
| B — baselines | recorded derived facts, opt-in | "has this moved by accident" | ms |

Tier I is the `SlEvent`-in / `SlCommand`-out seam: everything downstream
of the network reads `SlEvent(SessionEvent)` messages and everything
outbound is a `SlCommand`, so a fixture world needs no socket. Tier F is
for what only grid *sequencing* can break: arrival ordering, the CAPS
fetch paths, teleport / crossing hand-overs, `KillObject` timing,
multi-region offsets, in-flight asset leaks, NPC appearance delivery.

## Shared foundations

- `sl-client-bevy-viewer/src/viewer_plugins.rs` — the six plugin groups
  (`ViewerUiPlugins`, `ViewerInputPlugins`, `ViewerWorldPlugins`,
  `ViewerEditPlugins`, `ViewerRenderPlugins`, `ViewerShellPlugins`) shared
  by `run()`, the readback rig, `WorldTest` and the full-stack harness.
  A plugin never appears in two lists; a headless app takes the ECS half
  of a split plugin (`PickRegistryPlugin`, `PieMenuCorePlugin`).
- `sl-client-bevy-viewer/src/pixel_oracle.rs` — `Frame`, `Projected`,
  verdicts and the colour/coverage/luminance/difference oracles. Pure
  functions with synthetic-frame teeth tests.
- `sl-test-assets` — procedural pixels and geometry (checkers, solids,
  JPEG2000 encoding, a unit cube mesh, mini avatar bakes) consumed by both
  the no-grid `SceneAssets` and the fake grid's asset source, so "the
  checker is red/green" means the same thing in every tier.
- `sl-viewer-world-view/src/quiescence.rs` — `scene_is_quiet` (asset
  managers' in-flight counts) and the readback rig's `settle()`
  (pipelines compiled, probe bursts complete, timeline reached, clock
  held). A capture taken before the scene settles is a flake, not a test.
- `sl-viewer-testkit/src/baseline.rs` — the one baseline format for UI
  and render facts (`baselines/<crate>/<tier>/<id>.toml`, bless with
  `SL_VIEWER_BLESS_BASELINES=1`). A tier keeps its own opt-in list of
  baselined subjects, each with a reason, and asserts
  `baseline::orphans(krate, tier, &known)` is empty so a recording cannot
  outlive its subject. Record only what does not depend on the machine: a
  pie label's angle is the widget's maths, its radius is the font's
  metrics.

## Rules every check follows

- **No golden images.** Pixel-exact comparison across drivers turns the
  suite into a driver-version detector. Assert what is decidable: where a
  known colour landed (dominant-channel classification at projected
  points), coverage of a projected silhouette, luminance orderings against
  a calibration plate, A/B differences between an effect on and off,
  symmetry where the geometry declares it.
- **Teeth.** Every new check ships a paired test proving it fires on a
  known-bad case and stays silent on the good one. A check that cannot be
  shown to bite is decoration.
- **Expected verdicts are rules, not per-cell tables.** A subject under
  an opaque occluder is `Hidden`; under a translucent one `SeeThrough`;
  otherwise `Visible`. A scene overrides a rule only with a `reason`.
- **Determinism before serialisation before exclusion.** GPU tests stay in
  the pre-commit `nextest` run: the `gpu` test-group in
  `.config/nextest.toml` runs one GPU app at a time and `gpu_lock()`
  serialises plain `cargo test`; the rig disables pipelined rendering,
  drives time with `TimeUpdateStrategy::ManualDuration`, and holds the
  clock for the captured frame. Wall time is not a design constraint yet
  — long pre-commit test runs get dealt with when they actually become a
  problem, not before; the generated all-pairs sweep is the one opt-in
  exception (`SL_VIEWER_RENDER_MATRIX=full`). To keep that judgement
  evidence-based, ggh's check-timing report (`ggh timings`) displays the
  last measured duration for each check, and deliberately never
  overwrites a non-skipped number with a skipped run's number — so the
  recorded cost of a test stays the cost of actually running it.
- **Capture budgets are a registry guard.** A sweep declares its capture
  budget and the registry test fails when a new subject or axis exceeds
  it — before anyone pays the GPU time.

## How to add …

- **a render subject**: register a `RenderScene` with `subject:
  Some(SubjectSignature { marker, points, bounds, translucent, emissive })`
  and the `applies` axes; the R0 canonical sweep and every listed R1 axis
  pick it up. A self-staged scene (`subject: None`) is captured with the
  bare rig and takes no contexts.
- **a context axis**: a variant on `ContextSet`, a stage builder in
  `render_stage.rs`, an `expectation()` rule, and a `toggle_should_differ`
  row if it is a toggle.
- **an interaction test**: `InteractionTest` (UI) or `WorldTest` (world)
  from the testkit / viewer crate; drive the pointer with
  `hover`/`click`/`drag`/`type_str`; assert effects via `Recorded<M>`
  drains and entity queries, never via widget internals. The fixture
  world comes in folds — `world_app` (world group only),
  `…_with_edit`, `…_with_hud`, `…_with_input` (the camera / action /
  movement group), `…_with_ui`, `…_with_ui_and_edit`,
  `…_with_ui_and_input` — and a test takes the smallest one that can
  produce its failure. A `Recorded<M>` drain lags the write by a frame —
  the copying system is an unordered `Update` system — so step one update
  past the input that produced the message, or an effect that did happen
  reads as "the key did nothing".
- **a fake-grid fixture**: a `PrimFixture`/`NpcFixture`/`TerrainFixture`
  in `sl-fake-grid/src/fixtures/`, named in the catalogue so the viewer
  harness, the conformance `Grid::Fake` branch and the Firestorm
  cross-check binary all see the same region.
- **a full-stack test**: `ViewerHarness::start(fixture).login()`, wait on a
  timeline `Marker` and `wait_quiet()`, `capture()` and read it with the
  pixel oracles; return `Ok` with a log line when no adapter is present.
