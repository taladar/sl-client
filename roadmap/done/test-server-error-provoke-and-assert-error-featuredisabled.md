---
id: test-server-error
title: provoke and assert Error / FeatureDisabled
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 19 — Error handling & recovery `[both]`
---

Context: [context/test.md](../context/test.md).

`server-error` — provoke and assert `Error` / `FeatureDisabled`. `1av`.
**Pass (partial) on BOTH grids — neither message is provokable live.**
Investigated 2026-08-12:

- **Stock OpenSim never sends either message.** A grep over the whole
  OpenSimulator C# tree finds no constructor/sender for the `Error`
  (Low 423) or `FeatureDisabled` (Low 19) packets anywhere — not in
  `LLClientView`, not in any region module. (The packet classes exist only
  in the bundled libomv DLL.) There is nothing to provoke on the local
  grid.
- **Second Life silently drops the deprecated message that should trigger
  the blacklist response.** The reference viewer keeps a `FeatureDisabled`
  handler (`process_feature_disabled_message`, log-only: "Blacklisted
  Feature Response") and an `Error` handler
  (`LLMessageSystem::processError`, also log-only), so the plausible
  deterministic provocation was a message SL has deprecated: the legacy
  UDP `FetchInventoryDescendents` (superseded by
  `FetchInventoryDescendents2`/AISv3). Live on aditi the probe drew **no
  reply of any kind** within 30 s — no `FeatureDisabled`, no `Error`, and
  no descendents reply — confirming the runtime-docs claim that SL's UDP
  inventory fetch simply goes unanswered.

The case (`sl-conformance/src/cases/server_error.rs`) therefore records the
grid's honest answer to the probe: it hand-builds the raw
`FetchInventoryDescendents` for the agent's own inventory root and sends it
via `Command::Send` (bypassing the runtime's cap-preferring inventory
router, which would re-route the fetch over CAPS on both grids), then
resolves one of four outcomes: `FeatureDisabled` (assert non-empty message,
record whether `AgentID` matches self) or `Error` (assert non-empty
message, record code/token/system/message) both pass complete;
a normal `InventoryDescendents` reply or silence records `reply_kind` and
marks partial — except silence on OpenSim, which fails (OpenSim
demonstrably serves UDP inventory, so silence there is a real anomaly).
Observed: OpenSim answers the deprecated fetch normally
(`reply_kind = "inventory_descendents"`, 6 root folders, ~70 ms); aditi
ignores it (`reply_kind = "none"`).

If either grid ever starts answering with the real messages, the case
upgrades itself to the complete assertions with no code change.

**Decode is covered in-process** rather than live: the client ↔
`SimSession` round-trip `session_error_and_feature_disabled_reach_client`
(`sl-proto/tests/sim_session.rs`) already drives both messages through the
server-side encoders into the client session and asserts the typed
[`Event::ServerError`] / [`Event::FeatureDisabled`] carriers field by
field, so the parse surface this roadmap item worried about is
deterministically exercised on every `cargo test`.

**New client code:** none in the protocol crates (the `Command::Send` raw
path and both events already existed). The conformance harness gained a
`Session::session_id()` accessor (`sl-conformance/src/context.rs`) so a
case can fill the `AgentData` block of a hand-built wire message.
