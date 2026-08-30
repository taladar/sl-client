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
