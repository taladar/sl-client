---
id: viewer-automation-protocol
title: sl-automation-proto — the locator, request and snapshot vocabulary
topic: viewer
status: ready
origin: viewer automation design (2026-09-28)
points: 5
refs: [test-crosscheck-ui-scenes, test-firestorm-automation-endpoint]
---

Context: [context/automation.md](../context/automation.md).

Everything that drives a viewer — the in-process transport, the remote one,
the driver library, the CLI, and one day the patched Firestorm — has to agree
on one vocabulary. It is pure data, so it is its own crate with no Bevy in it.

## Wanted

A new `sl-automation-proto` crate (serde, no runtime) holding the **core**:

- `Locator`: a role (`button`, `checkbox`, `textbox`, `combobox`, `slider`,
  `tab`, `menuitem`, `listitem`, `window`, `text`, …), a name matcher (exact,
  substring) against the resolved text **or** a Fluent key (`name_key`), a
  `test_id` (the entity's `Name`, sharing the address space of
  `ui_contract.rs`), `within` (another locator as scope), `nth`, and state
  filters (`enabled`, `checked`, `selected`, `expanded`, `focused`).
- `UiNode`: role, name, name key, test id, states, value, bounds (logical
  px), visibility.
- `Request` / `Response` for "snapshot", "find", "click", "fill", "wait
  for", with ids so several may be in flight.
- Error kinds a test can match on: not found, ambiguous (with candidates),
  not actionable (with the failing check), timed out (with the last
  observed state).

**It grows with its consumers.** The workspace denies dead code and keeps
no forward-looking API, so this task lands only what the first consumer
(the semantic model and locator engine) constructs; `WorldLocator` /
`WorldNode` arrive with [[viewer-automation-world-model]], probe requests
with [[viewer-automation-state-probes]], the selector string grammar with
[[viewer-automation-ctl-cli]], a protocol version with the remote
transport. Every variant has a producer and a consumer when it lands.

Selectors are a per-viewer namespace ([[test-firestorm-harness-skin-selection]]
settled that): the *types* are shared, but a Fluent key or a test id means
something only to the viewer that owns it.

New crate: expect the extraction gates (`private_interfaces`,
`must_use_candidate`, fmt, machete, cargo-about, rustdoc, `cliff.toml`,
`CHANGELOG.md`) — see the new-crate memory.

Acceptance: the crate builds with no Bevy dependency; every request and
response round-trips through JSON; nothing in it is unused.
