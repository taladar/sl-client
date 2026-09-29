---
id: test-e2e-viewer-process-launch
title: Extract viewer process launch and graceful stop from sl-crosscheck
topic: test
status: done
origin: viewer automation design review (2026-09-28)
points: 3
refs: [test-firestorm-crosscheck-runner, test-e2e-stage]
---

Context: [context/automation.md](../context/automation.md).

`sl-crosscheck/src/{launch,process}.rs` already solve launching a viewer
safely: every `XDG_*` root confined to the run directory (the viewer
rewrites its settings on exit), `BEVY_ASSET_ROOT` pinned, and stopping by
`SIGTERM` → logout grace → `SIGKILL`, because a killed viewer strands its
session and the *next* login fails. The end-to-end stage needs exactly
this, for several viewers at once.

## Wanted

- Move the launch and stop logic into a shared library module (a small
  crate, or a module of the future `sl-e2e`, whichever the gates make
  cheaper) that both `sl-crosscheck` and the stage use.
- Launch options it gains for the stage: extra arguments
  (`--headless`, `--automation-socket`), an environment block, a per-viewer
  run directory, and several children alive at once, each stopped
  gracefully and in parallel.

Acceptance: `sl-crosscheck` behaves exactly as before on top of the shared
code; a test launches two viewer processes, stops both gracefully, and
neither leaves a session on the grid.

## Outcome (2026-09-29)

- A small crate, **`sl-viewer-launch`** (fs-err, nix, signal-hook — no
  Bevy, no grid), rather than a module of `sl-e2e`: the stage will depend on
  the driver and the viewer, and `sl-crosscheck` should not.
  - `Launch` (name, program, args, env, log; `arg`/`args`/`env`/`envs`
    builders) and `Launch::in_dir(name, program, &ViewerDir)`.
  - `ViewerDir` — one viewer's run directory, made absolute: `state/` (the
    four `XDG_*` roots via `confined_env`) and `viewer.log`.
  - `RunningViewer::spawn` — several alive at once; `try_ending`, `wait`,
    `stop(grace)` (`SIGTERM` → grace → `SIGKILL`, a viewer that already
    ended is only reported, never signalled). **Dropping a running one stops
    it the same way**, so a panicking test body still logs every viewer out.
  - `stop_all(viewers, grace)` stops them in parallel, one thread each: N
    viewers wait out one grace, not N.
  - `run`, `interrupt_flag`, `is_executable`, `LOGOUT_GRACE` — the one-viewer
    path `sl-crosscheck` uses, unchanged in behaviour.
- `sl-crosscheck`'s `Launch` keeps its `viewer` and `artefacts` and carries
  the shared `process: sl_viewer_launch::Launch`; its `process.rs` is gone,
  and `nix` / `signal-hook` left its manifest with it.
- Acceptance: `sl-client-bevy-viewer/tests/viewer_processes.rs` launches the
  built binary twice `--headless`, each confined to its own `ViewerDir`, into
  one fake grid as two accounts; waits for both sessions in the region, stops
  both with `stop_all`, and asserts each ended `AskedToQuit`, each session
  closed with `ServerEvent::LoggedOut`, and the region holds no session. It
  runs in the nextest `gpu` group (`binary(viewer_processes)`); ~3 s in a
  release build.
