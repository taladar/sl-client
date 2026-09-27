---
id: test-e2e-viewer-process-launch
title: Extract viewer process launch and graceful stop from sl-crosscheck
topic: test
status: ready
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
