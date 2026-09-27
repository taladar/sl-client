---
id: test-e2e-live-grids
title: Run end-to-end tests against the local OpenSim and aditi
topic: test
status: blocked
origin: viewer automation design (2026-09-28)
points: 5
blocked_by: [test-e2e-stage]
refs: [server-world-chat-routing, server-world-agent-movement]
---

Context: [context/automation.md](../context/automation.md).

The fake grid does not yet relay chat between sessions, show real avatars
to each other, or move them — and it is a separate track to make it do so.
A test that needs any of that should still be writable today, and some
behaviour must be checked against a real grid regardless.

## Wanted

- `Stage` against a live grid, chosen by environment
  (`SL_E2E_GRID=fake|opensim|aditi`): accounts from `credentials.toml` /
  `credentials.aditi.toml` by avatar key, no fake-grid control handle.
- Tests declare what they need (grid control, N accounts, a named region or
  content) and skip with a stated reason when the chosen grid cannot
  provide it.
- The aditi per-avatar cooldown and ban caution shared with
  `sl-conformance` (one guard, not two), and live runs never part of the
  pre-commit suite.

Acceptance: one pilot test that needs no grid control passes on the fake
grid and on the local OpenSim unchanged; a test needing grid control skips
on OpenSim with its reason.
