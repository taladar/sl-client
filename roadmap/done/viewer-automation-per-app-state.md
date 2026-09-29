---
id: viewer-automation-per-app-state
title: Per-App state — two logged-in viewers in one process
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 8
blocked_by: [viewer-automation-app-builder]
refs: [viewer-automation-inprocess-transport]
---

Context: [context/automation.md](../context/automation.md).

The in-process backend runs several viewers as several Bevy Apps in one test
process. Several pieces of state are process-wide today and would be shared,
or fought over, between them:

- `sl-viewer-platform/src/paths.rs`: `STARTUP_OVERRIDES`,
  `MEDIA_ENGINE_PROFILE`, `REPLAY_CACHE_ROOT`;
- `sl-viewer-world-view/src/session.rs`: `TERMINATION_REQUESTED` (the
  SIGTERM flag — one signal must log out every App in a test process, but a
  test must also be able to log out one viewer);
- env-var `OnceLock` switches in `sl-viewer-world-scene` and
  `sl-viewer-world-view` (read once per process, so two viewers cannot
  differ);
- the process-wide static-asset library (`install_static_assets`).

## Wanted

- Move whatever can legitimately differ between two viewers into per-App
  resources set by `ViewerAppBuilder`; keep env vars as the *default* the
  builder reads, not a global the systems read.
- Document, in the owning modules, what stays deliberately shared and why
  (`SHARED_RUNTIME`, `FETCH_CLIENT`, the static-asset library, the GPU lock).
- A test that builds two Apps, logs both into one fake grid as two accounts,
  and checks each sees its own agent, directories and settings.

Also settle, and write down, the process-wide things Bevy itself owns: the
tracing subscriber is global (`LogPlugin` must be off in every App, logs
routed per App by a span field), the task pools are shared, and each App
opens its own wgpu device (measure the memory of two).

Acceptance: that two-App test passes; changing a per-viewer option on one
App does not change the other; each App's log lines are attributable to it;
the interactive viewer is unaffected.
