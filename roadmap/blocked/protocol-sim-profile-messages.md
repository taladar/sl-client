---
id: protocol-sim-profile-messages
title: SimSession — decode profile requests and send profile replies
topic: protocol
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-profiles]
---

Context: [context/protocol.md](../context/protocol.md).

## What

Avatar properties, interests, picks, classifieds and notes have no server-side
decode or senders. Add them.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
