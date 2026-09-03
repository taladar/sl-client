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
- `sl-fake-grid` is reproducible on demand:
  `FakeGridBuilder::deterministic(seed)` seeds every minted identifier (session,
  secure session, circuit code, capability tokens, agent and region ids) and
  `FakeGridBuilder::clock(now)` replaces every grid-side stamp — nothing in the
  crate reaches for `Instant::now()` on its own, and
  `sl_fake_grid::tokio_clock()` is what a paused-timer test passes. Tier F
  records the grid produces are therefore comparable run to run.
- `sl-crosscheck` — the Firestorm cross-check runner: one in-process fake
  grid on a fixed port with a named scenario, both viewers run against it
  in turn with the same capture size, layers, camera and day position, and
  a run directory of frames, scene dumps, `harness-status.json` and
  `run.json`. The runner collects and never looks at a pixel; the
  comparison is its own binary, `sl-crosscheck-report <run>`, which writes
  a contact sheet, a difference image per frame and a scene-dump diff into
  `<run>/report`. It is developer-facing and never a gate. Both viewers write
  `harness-status.json` before logging out (ours is
  `sl-viewer-world-view/src/harness_status.rs`), because a viewer that
  never got in world still writes a full set of black frames on schedule —
  "the run did not happen" and "the viewers differ" must never read the
  same way. A viewer is asked to quit (`SIGTERM` → logout grace →
  `SIGKILL`), never killed outright: a stranded grid session makes the
  *next* run fail to log in.
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
  `…_with_ui_and_input`, `…_with_ui_and_inventory` (the real inventory
  window, for the drag-and-drop flow) — and a test takes the smallest one
  that can produce its failure. A `Recorded<M>` drain lags the write by a
  frame — the copying system is an unordered `Update` system — so step one
  update past the input that produced the message, or an effect that did
  happen reads as "the key did nothing".
- **a drag-and-drop test**: `drag_drop_tests` in `world_test.rs`. The drop
  target is whatever the **world tier's own drag pick** last resolved
  (`DragPickActive` / `DragWorldPick`), not a ray cast from the drop
  observer, and that pick runs at ~15 Hz off the 16 ms fixture clock and
  answers a frame later — so a drag rests on its target for a couple of
  dozen frames before it releases or is read. That rest also carries it
  past the frame a prim re-tessellates with no faces at all
  ([[viewer-prim-rebuild-drops-a-click]]), where a pick answers `None`.
- **a context-menu dispatch test**: open the pie with a **real
  right-click** (`world_test::right_click_at`), so the stashed target is
  the one the classifier resolved, then write the `UiAction` the slice
  emits rather than clicking its label — the label→action half is already
  pinned by the per-menu compass-address tables and, end to end, by
  `a_pie_slice_clicked_in_world_sends_its_command`. `pie_dispatch_tests`
  in `world_test.rs` is the worked example.
- **a fake-grid fixture**: a `PrimFixture`/`NpcFixture` in
  `sl-fake-grid/src/fixtures/`, named in the catalogue so the viewer
  harness, the conformance `Grid::Fake` branch and the Firestorm
  cross-check binary all see the same region. The ground is not scenario
  content but region content: `RegionConfig::terrain` carries a
  `TerrainFixture` (`sl-fake-grid/src/terrain.rs`) whose `Heightfield`
  answers both the LAND patches of the arrival burst and the estate RAW
  download, so "the ground is at 25 m" means the same thing on both
  paths.
- **a full-stack test**: `ViewerHarness::start(fixture).login()`, wait on a
  timeline `Marker` and `wait_quiet()`, `capture()` and read it with the
  pixel oracles; return `Ok` with a log line when no adapter is present.
