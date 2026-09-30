# sl-e2e

The end-to-end test stage: one in-process `sl-fake-grid` (or a live grid),
several real viewers logged into it, and the handles a test drives them with. A
test names its viewers and hands `StageBuilder::run` an async body:

```text
StageBuilder::new("floater_and_marker")
    .viewer_binary(env!("CARGO_BIN_EXE_sl-client-bevy-viewer"))
    .viewer("Alpha")
    .viewer("Beta")
    .run(async |stage: &Stage| {
        stage.viewer("Alpha")?.open_floater("inventory").await?;
        stage.mark("Beta", "opened").await?;
        stage.wait_marker("Beta", "opened", WAIT).await?;
        Ok(())
    })
```

- **Backends**: each viewer is the real binary (`--headless
  --automation-socket`, confined to its directory through `sl-viewer-launch`)
  or the viewer's own builder's App on an in-process host.
  `SL_E2E_BACKEND=process|in-process|both` (unset: both) runs the body once
  per backend on a fresh grid.
- **Every viewer logs in** as the account `Stage <label>` and settles before
  the body starts. The body gets an `sl-viewer-driver` handle per viewer, the
  grid (`Stage::grid`), each viewer's grid session (`Stage::agent`) and
  markers (`Stage::mark`, `Stage::wait_marker`).
- **Grids**: `SL_E2E_GRID=fake|opensim|aditi` (unset: a fresh fake grid per
  backend). On a live grid the viewers log in as the accounts of its
  credentials file (`SL_E2E_CREDENTIALS`, default the workspace's
  `credentials.toml` / `credentials.aditi.toml`), taken in the order
  `SL_E2E_AVATARS` gives (default `primary`, `secondary`, `tertiary`, then
  the rest), at `SL_E2E_START` (default: OpenSim's `Default Region` centre,
  aditi's `last`). There is no grid-control handle there, so a test states
  what it needs (`StageBuilder::needs(Need::GridControl)`,
  `Need::Content(..)`; naming regions or configuring the grid implies it)
  and is skipped with its reason where the grid cannot provide it, or where
  the file has fewer accounts than the stage has viewers. aditi logins wait
  out the per-avatar login cooldown `sl-conformance` shares
  (`sl_repl::LoginCooldown`), and an in-process viewer answers aditi's second
  factor through the avatar's `mfa_command`. Run a live grid under
  `cargo nextest run --profile live`: the default profile kills a test after
  12 minutes, and a killed test strands its avatars' sessions on the grid.
- **Teardown always runs**, also after a failed or panicking body: every
  viewer is asked to log out, and the run fails if one would not or if the
  fake grid still holds a session afterwards.
- **Artifacts** land in `<target>/e2e/<test>/<grid>/<backend>/`: `grid.log`, and
  per viewer `viewer.log`, `failures/` (the driver's failure artifacts) and
  `state/`.
- A machine with no GPU adapter skips a stage with a warning.
