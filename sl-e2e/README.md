# sl-e2e

The end-to-end test stage: one in-process `sl-fake-grid`, several real
viewers logged into it, and the handles a test drives them with. A test
names its viewers and hands `StageBuilder::run` an async body:

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
- **Teardown always runs**, also after a failed or panicking body: every
  viewer is asked to log out, and the run fails if one would not or if the
  grid still holds a session afterwards.
- **Artifacts** land in `<target>/e2e/<test>/<backend>/`: `grid.log`, and per
  viewer `viewer.log`, `failures/` (the driver's failure artifacts) and
  `state/`.
- A machine with no GPU adapter skips a stage with a warning.
