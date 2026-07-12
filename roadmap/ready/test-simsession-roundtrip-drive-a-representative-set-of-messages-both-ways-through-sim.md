---
id: test-simsession-roundtrip
title: drive a representative set of messages both ways through SimSession an
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase 20 — Server side (SimSession) — stretch, no grid
---

Context: [context/test.md](../context/test.md).

Optional final tier: in-process client ↔ `SimSession` round-trips for messages
that are hard to provoke against a live grid. Complements
`sl-proto/tests/sim_session.rs`. These are not grid-gated.

`simsession-roundtrip` — drive a representative set of messages both ways
through `SimSession` and assert symmetric decode/encode.

---
