---
id: protocol-sim-profile-messages
title: SimSession — decode profile requests and send profile replies
topic: protocol
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
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

The measured behaviour is in `book/src/gridspec/profiles.md` (the
`profiles` census): every UDP request and reply both grids answer, which
replies OpenSim volunteers after an edit (the profile and interests after
`AvatarPropertiesUpdate`, the empty picks list after `PickDelete`) and
Second Life does not, and the generic requests' NUL-terminated strings.
