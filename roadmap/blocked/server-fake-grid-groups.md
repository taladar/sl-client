---
id: server-fake-grid-groups
title: Fake grid — a group store: membership, roles, notices, invitations and group sessions
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-groups, gridspec-group-chat-and-conference,
  protocol-sim-group-messages]
---

Context: [context/server.md](../context/server.md).

## What

No group exists on the fake grid. Store groups, members, roles and notices; fan
out group session messages; each grid's quirks as flavour rows (activation,
`AgentDropGroup`, name cap, propagation delay).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
