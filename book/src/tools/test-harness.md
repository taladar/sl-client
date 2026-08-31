# The viewer test harness

The Bevy viewer is verified by an automated harness in `cargo test` /
`cargo nextest`, organised in tiers. The rule: **a test lives in the
lowest tier that can produce its failure.** A live run against a grid is
the exception, for behaviour no tier reaches yet.

| Tier | Where | What it sees |
| --- | --- | --- |
| Geometry | `sl-client-bevy-viewer/src/render_test.rs` | meshes, samplers, transforms, logs — no GPU |
| UI layout | `sl-viewer-testkit` (`LayoutTest`) | bevy_ui layout with real fonts — no renderer |
| Interaction | `sl-viewer-testkit::interact` (`InteractionTest`), viewer `world_test.rs` (`WorldTest`) | synthetic pointer/keyboard into the UI and into a headless fixture world |
| Render matrix | `sl-client-bevy-viewer/src/render_readback.rs`, `render_matrix.rs` | pixels of registered scenes under context axes (eye, time of day, mirror, layering, toggles, HUD) |
| Full stack | `sl-client-bevy-viewer/src/full_stack_test.rs` + `sl-fake-grid` | the real client stack against an in-process grid, read back as pixels |
| Baselines | `sl-viewer-testkit::baseline`, `baselines/` | recorded derived facts that may not drift by accident |

The design, the shared foundations and the rules every check follows are
in `roadmap/context/testing.md`; this chapter documents how to run and
extend the harness and fills in as the tiers land.

## Running

- Everything: `cargo nextest run --workspace`. GPU tests run in the `gpu`
  test-group defined in `.config/nextest.toml`, one GPU app at a time;
  plain `cargo test` serialises them through `gpu_lock()`.
- The all-pairs render sweep is opt-in:
  `SL_VIEWER_RENDER_MATRIX=full cargo nextest run -P gpu-full -p
  sl-client-bevy-viewer`.
- A machine without a GPU adapter skips the GPU tiers with a logged
  warning; it never fails them silently.

## Rules

- No golden images. Assert where a known colour landed, coverage of a
  projected silhouette, luminance orderings, A/B differences, symmetry.
- Every check ships a teeth test: it fires on a known-bad case and stays
  silent on the good one.
- Expected verdicts are rules (opaque in front → hidden; translucent in
  front → see-through), not per-cell tables.
- A capture is taken only once the scene has settled.

## Baselines

The tiers above catch what is **wrong**. A baseline catches what merely
**moved**: the vertex count a box tessellates to, the angle a pie option
sits at, where a subject's centre lands in the frame. None of those is
incorrect at any particular value, a refactor moves them for free, and a
user who has opened the same menu ten thousand times notices.

One format serves every tier, in `sl-viewer-testkit::baseline`:

- A subject's facts live in `baselines/<crate>/<tier>/<id>.toml` — one
  file per subject, so two subjects moving in two commits never
  conflict.
- A fact is an `Int`, a `Text`, or a `Float` / `Vec2` / `Vec3` that
  carries the tolerance it is compared at, so the file itself says how
  exact each number is meant to be.
- A run builds `Facts` and calls `baseline::check_subject(krate, tier,
  id, facts)`. A drift fails with every moved fact named; a missing file
  fails naming the bless command, never blessing itself.
- `SL_VIEWER_BLESS_BASELINES=1 cargo test …` rewrites the files it
  checks. The diff that moves a fact and the diff that blesses the move
  belong in the **same commit** — the review moment is the entire point.
- Each tier keeps a list of its baselined subjects and asserts
  `baseline::orphans(krate, tier, &known)` is empty, so a recording
  cannot outlive the subject it describes.

What is baselined is **opt-in** and says why. Recording everything makes
every intended change a noisy diff, and a noisy check gets skimmed, then
re-blessed unread, then deleted. Record *derived intent* — counts,
extents, angles — never raw dumps: a vertex-position dump changes
whenever a float does and teaches everyone to re-bless without reading.
Record the resting cell, not the whole matrix; the tiers above cover the
rest. And record only what does not depend on the machine: a pie label's
*angle* is the widget's own maths, while its *radius* grows with the
measured text and would make the file a font-version detector.

Landed so far: ten render scenes (per-LOD vertex and triangle counts,
world extents, and the CPU-projected framing pixel of the subject's
centre — held to the readback rig's own camera by
`the_cpu_framing_projection_agrees_with_the_rendered_camera`), and the
pie menu's measured compass angles.
