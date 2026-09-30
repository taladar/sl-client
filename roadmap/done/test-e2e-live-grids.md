---
id: test-e2e-live-grids
title: Run end-to-end tests against the local OpenSim and aditi
topic: test
status: done
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [test-e2e-stage]
refs: [server-world-chat-routing, server-world-agent-movement]
---

Context: [context/automation.md](../context/automation.md).

The fake grid does not yet show real avatars to each other, or move them —
and it is a separate track to make it do so. A test that needs any of that
should still be writable today, and some behaviour must be checked against a
real grid regardless.

## Wanted

- `Stage` against a live grid, chosen by environment
  (`SL_E2E_GRID=fake|opensim|aditi`): accounts from `credentials.toml` /
  `credentials.aditi.toml` by avatar key, no fake-grid control handle.
- Tests declare what they need (grid control, N accounts, a named region or
  content) and skip with a stated reason when the chosen grid cannot
  provide it.
- The aditi per-avatar cooldown and ban caution shared with
  `sl-conformance` (one guard, not two), and live runs never part of the
  pre-commit suite.

Acceptance: one pilot test that needs no grid control passes on the fake
grid and on the local OpenSim unchanged; a test needing grid control skips
on OpenSim with its reason.

## Outcome (2026-09-30)

- **`SL_E2E_GRID=fake|opensim|aditi`** (`sl_e2e::Grid`, unset: the fake
  grid, so the commit hook never logs into a live one). On a live grid the
  stage starts no grid: the viewers log in as the accounts of the grid's
  credentials file (`SL_E2E_CREDENTIALS`, default the workspace's
  `credentials.toml` / `credentials.aditi.toml`) in `SL_E2E_AVATARS` order
  (default `primary`, `secondary`, `tertiary`, then the rest), at
  `SL_E2E_START` (default OpenSim's `Default Region` centre, conformance's
  spot, so every viewer hears the others; aditi's `last`). The home region
  and agent ids are read from the agent probe once each viewer has arrived;
  `Stage::account_name` is who a viewer is heard as.
- **Needs** (`sl_e2e::Need`): `GridControl` (`grid`, `agent`, `mark`,
  `wait_marker` answer `NoGridControl` on a live grid), `Content(..)`, and
  `DictatedGrid`, implied by `region`, `configure_grid` and
  `start_position`. A grid that cannot meet them, or has fewer accounts than
  the stage has viewers, is skipped with a warning naming why, before any
  credentials file is read or login made (`StageBuilder::skip_reason`).
- **One cooldown guard**: `sl_repl::LoginCooldown`, stamps under
  `$XDG_STATE_HOME/sl-client/login-cooldown/` — shared by `sl-conformance`
  (refuses; its own copy is gone) and the stage (waits), and by every
  worktree. A stage on aditi waits the window out between its backends.
- **aditi's second factor in process**: the App ends on the challenge; the
  stage reads `LoginOutcome` from the exited App, answers with the avatar's
  `mfa_command` and logs in again with a new App. Not exercised live: aditi
  issued no challenge in any of today's runs.
- **Live runs** go under `cargo nextest run --profile live`, which never
  kills a test (a killed test strands its avatars), and a live viewer gets
  ten minutes to settle.
- **Pilot tests**: the chrome and chat tests need nothing but accounts; the
  pie declares `GridControl`, the two-viewer prim `Content` (the stock box),
  the teleport dictates its regions. In the stage suite the marker and the
  killed-viewer tests declare `GridControl`.
- **Acceptance**: on the local OpenSim the chrome and two-viewer chat tests pass
  unchanged on both backends and the other three skip with their reasons;
  `the_status_bar_names_the_region_and_the_menu_bar_opens_the_build_window` also
  passes on aditi on both backends. A unit test checks that a grid-control test
  skips on a live grid without running its body.

Found on the way:

- **The mesh store left rigged and physics-fetched meshes "downloading"
  forever**: `get_skin` / `get_physics` pass the entry through
  `Downloading` for the header and blocks and never published a terminal
  progress, so 63 aditi meshes were counted as in-flight downloads by the
  pipeline overlay and the quiet wait for good. They now settle back.
- **A quiet wait that times out now says what is not quiet**: the
  `Quiescence` readout carries `outstanding_by`, the work by bucket, which is
  how the mesh bug was told apart from aditi's asset service answering
  textures 503 for minutes (legitimate retry chains).
