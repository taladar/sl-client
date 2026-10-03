---
id: server-fake-grid-friends-presence
title: Fake grid — a mutable friendship store with rights, presence fan-out and calling cards
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-friends-presence]
---

Context: [context/server.md](../context/server.md).

## What

Only a static buddy list exists. Offer / accept / decline / terminate, rights
grants with SL's masking, online / offline fan-out on session open and close,
calling cards.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.
