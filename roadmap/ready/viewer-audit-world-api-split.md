---
id: viewer-audit-world-api-split
title: sl-viewer-world-api is a 6892-line god-module and the workspace's shared-types dump
topic: viewer
status: ready
origin: static code audit (2026-08-26)
points: 13
refs: [viewer-audit-world-api-query-tests]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-api/src/lib.rs` is one file: 6892 lines, 168 `pub` items, 25
`Resource`s, **zero submodules and zero traits**, 5 tests.

What it genuinely abstracts is `WorldPhase` — a 10-variant `SystemSet` — and
that *is* honoured: every variant has both producers and consumers across the
layers. Everything else is shared concrete state.

Worse, it carries whole domains with nothing to do with the world:
`FriendsModel`, `GroupsModel`, `MuteModel`, `PresenceState`, and the notecard /
script / conference / browser open events. Their consumers are
`sl-viewer-people` (30 refs), `sl-viewer-notices` (8), `sl-viewer-inventory` (5)
and `sl-viewer-edit` (5) — against **four total** from the five world crates. So
14 unrelated crates depend on the world layer to reach them.

Scope, in two steps:

1. Split along the ten banner-delimited sections the file already carries
   (`Cross-tier intents`, `Settings the behaviour reads`, `Object flag bits`,
   `Ordering phases`, ...) — they are ready-made module boundaries nobody took.
2. Lift the social/intent half into its own crate (`sl-viewer-social` or
   `sl-viewer-intents`), which removes the world-layer dependency from those 14
   crates.

This is the highest-leverage structural change available, and it unlocks the
largest testability win in the audit — see
[[viewer-audit-world-api-query-tests]].

Two small companions in the same spirit: all four downstream crates open with a
blanket `#![expect(clippy::module_name_repetitions)]` and a block of
`pub(crate) use ... as ...` aliases (e.g.
`sl-viewer-world-scene/src/lib.rs:23-46`, 24 of them) so call sites keep saying
`crate::coords`. The layering is real in `Cargo.toml` — a clean DAG, api then
objects then avatar/scene then view — but **invisible in the source**, so no
file reads as crossing a crate boundary and nothing resists a new reach-across.
