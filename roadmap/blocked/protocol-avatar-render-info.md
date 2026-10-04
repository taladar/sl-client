---
id: protocol-avatar-render-info
title: AvatarRenderInfo — client request, SimCaps service and fake-grid answers
topic: protocol
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-avatar-render-info]
---

Context: [context/protocol.md](../context/protocol.md).

## What

The cap exists nowhere in the workspace. Add the client request and reporting,
the server-side cap, and the fake grid's per-flavour answers.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Capability (triage 2026-10-04)

`AvatarRenderInfo` (Second Life only) is two halves. The GET (every 15 s)
returns `{agents:{<id>:{weight}}, reportinglimit, overlimit}` — the
server-reported complexity and the "N residents render you as a jellydoll"
notice; that half is a feature. The POST (every 60 s) reports
`{agents:{<id>:{weight, tooComplex}}}` for the avatars this viewer sees; it is
what other residents' notices are built from, so it is sent like the GET, not
behind [[viewer-telemetry-opt-in]].

Shapes and Firestorm references: `book/src/comms/caps-reference.md`; which grid
grants it: `book/src/gridspec/capabilities.md`.
