---
id: protocol-sim-group-messages
title: SimSession — decode group requests and send group replies
topic: protocol
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-groups]
---

Context: [context/protocol.md](../context/protocol.md).

## What

Group requests reach the server side only as raw `ClientMessage`s and
`SimSession` has no group profile / members / roles / notices senders. Add them,
loopback-tested like the other `protocol-sim-*` surfaces.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
