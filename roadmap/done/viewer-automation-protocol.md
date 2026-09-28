---
id: viewer-automation-protocol
title: sl-automation-proto — the locator, request and snapshot vocabulary
topic: viewer
status: done
origin: viewer automation design (2026-09-28)
points: 5
refs: [test-crosscheck-ui-scenes, test-firestorm-automation-endpoint]
---

## Done (2026-09-28)

`sl-automation-proto` exists: serde and thiserror, no Bevy, no runtime.
`Locator` (+ `NameMatcher`), `UiNode` (+ `Role`, `NodeState`, `NodeValue`,
`NodeVisibility`, `Bounds`, `NodeId`), `Request` / `RequestBody` /
`RequestId`, `Response` / `ResponseBody`, `WaitCondition`, `Deadline`,
`AutomationError` and `ActionabilityCheck`. Every variant round-trips through
JSON in the crate's tests. Choices the consumers inherit:

- **Wire shape.** A request is flat: `{"id":1,"method":"click","locator":…}`.
  A response carries `ok` or `error` beside the id, and the body or error
  inside is tagged by `kind`. Optional fields are left out when unset.
  `Locator` and `Deadline` refuse unknown fields, so a misspelt criterion is
  an error instead of a broader match.
- **`Role::Group`** is a container with no widget role of its own (a panel, a
  list body, a scroll area). It is kept in the tree so `within` has something
  to scope to. The semantic model should emit one for every scoping container
  a test needs, and not for every layout node.
- **States are a set.** `UiNode::states` is a `BTreeSet<NodeState>`, so a
  missing state means false, and a locator's `enabled` filter reads
  `Disabled` (which counts disabled ancestors). Name matching is
  case-sensitive (`Exact` / `Contains`).
- **`Locator::matches_node`** checks everything about a single node (role,
  name, key, test id, state filters). `within` and `nth` are left to the
  resolver that walks the tree, which is the locator engine's job.
- **`NodeVisibility`** is one value, the first reason a node cannot be seen,
  in this order: `Hidden`, `Clipped`, `OffScreen`, `Covered`. `Visible` is
  the only actionable one.
- **`ActionabilityCheck::Editable`** was added beyond the task text. `fill`
  needs a check that a read-only field fails.
- **`TimedOut`** covers both kinds of wait. A `wait_for` sets `condition`; an
  action waiting for actionability sets `failed_check`. Both carry
  `last_observed` and the frames and milliseconds waited.
- **`Find` is never strict.** Strictness belongs to the actions (`click`,
  `fill`). Their `Ambiguous` error lists every candidate.
- `Display` for `Locator` prints a description like
  `window #floater.x >> button key=ok` for messages. Nothing parses it; the
  selector grammar arrives with [[viewer-automation-ctl-cli]].

Nothing in the workspace depends on the crate yet: its first consumers are
[[viewer-automation-semantic-ui-model]] and
[[viewer-automation-synthetic-input]], both now ready.

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
