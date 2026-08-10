---
id: viewer-perf-login-state-gate
title: App-level login state to gate world/streaming systems pre-login
topic: viewer
status: ideas
origin: descoped from viewer-perf-run-condition-gating (2026-08-10)
refs: [viewer-perf-run-condition-gating, viewer-login-screen]
---

Context: [context/viewer.md](../context/viewer.md).

The [[viewer-perf-run-condition-gating]] pass gated closed-panel UI
refreshers, off-mode camera drivers, and debug/demo systems, but
deliberately left the world/streaming clusters (objects, terrain,
textures, sky/water, animation — the big `lib.rs` Update blocks)
running from frame 0. Reasons, verified at the time:

- No correct latch exists. `ViewerSession.agent_in_world` flips only
  when the own avatar object arrives — objects, terrain, and
  environment stream **before** that, so gating on it would stall or
  drop early streaming. The plugin's `SlState` is private and always
  present (`link: None` offline), so `resource_exists` can't key on it.
- Bevy messages expire after two frames; a state-latch gate over the
  message-consumer systems risks silently dropping events that arrive
  in the login window.
- Today the viewer starts logging in immediately, so the pre-login
  window is a few seconds — the win was not worth the correctness risk.

The right shape arrives with [[viewer-login-screen]]: once the viewer
has a real pre-login phase, introduce an app-level login `States`
machine (e.g. `LoginScreen` / `LoggingIn` / `InWorld`) whose
transitions are driven by the session plugin, and register the
world/streaming clusters under `run_if(in_state(InWorld))` (or
equivalent), with the "streaming active" flank set **before** the first
world event can be delivered so nothing is dropped. Cross-frame backlog
systems (`PendingObjectEvents`, `PendingDecodedMeshes` /
`PendingDecodedSculpts`, `PendingPatchRebuilds`,
`AvatarState.appearance_pending`) would need `is_empty()` accessors so
their gates also fire while a backlog remains.

## Estimated impact

Low until a login screen exists (the idle pre-login phase is where the
several-hundred-system dispatch floor is pure waste); becomes the main
lever for a cheap login screen afterwards.
