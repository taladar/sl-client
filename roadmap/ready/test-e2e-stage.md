---
id: test-e2e-stage
title: sl-e2e Stage — a fake grid, several viewers and grid control in one test
topic: test
status: ready
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-driver, test-e2e-viewer-process-launch]
refs: [server-fake-grid-scripted-avatars, viewer-fake-grid-render-harness]
---

Context: [context/automation.md](../context/automation.md),
[context/testing.md](../context/testing.md).

The piece a test starts with: bring up a grid and as many viewers as the
scenario needs, hand the test handles to all of them, and tear everything
down cleanly whatever happens. **Nothing here may assume one viewer.**

## Wanted

- `Stage` in a new `sl-e2e` crate: one in-process `sl-fake-grid`
  (`FakeGridBuilder` — regions, accounts, scenario, `deterministic(seed)`),
  N viewers, each logged in as its own account, and the grid-control
  handle (`FakeGrid`, `FakeAgent`, timeline, `mark` / `wait_marker`).
- Backend per viewer from `SL_E2E_BACKEND=process|in-process|both`
  (`both` runs the test once per backend); the process backend launches the
  real binary `--headless --automation-socket` through
  [[test-e2e-viewer-process-launch]].
- A slot for scripted avatars once [[server-fake-grid-scripted-avatars]]
  exists (the `sl-client-tokio` session it defines, not a new client).
- Per-test artifact directory under `target/e2e/<test>/<viewer>/`: viewer
  log, driver failure artifacts, the grid's event log.
- Tests live in `sl-client-bevy-viewer/tests/` so cargo builds the binary
  and exposes it as `CARGO_BIN_EXE_sl-client-bevy-viewer`.
- A machine with no GPU adapter skips loudly, as the full-stack tier does.
- A nextest `e2e` test-group beside `gpu` in `.config/nextest.toml`, with a
  bounded thread count and its own slow-timeout.
- `context/testing.md` gains tier **E — end-to-end** with the tier rule
  applied: a test belongs here only if nothing lower can produce its
  failure.

Acceptance: a two-viewer test on both backends logs both in, has one open a
floater while the other waits for a marker, and tears down with no stranded
sessions and no leftover processes, even when the test body panics.
