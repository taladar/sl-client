---
id: server-fake-grid-edit-permission-enforcement
title: Fake grid — refuse edits the editor may not make, as each grid refuses them
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-object-edit]
---

Context: [context/server.md](../context/server.md).

## What

Any session can edit any prim today. Enforce modify / move / copy rights with
each grid's refusal shape (silence, corrective update, alert).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured (2026-10-10)

[[gridspec-object-edit]] measured it with the conformance case
`object-edit-stranger` (two avatars, live only today) and wrote it up in
`book/src/gridspec/building.md` § Somebody else's object: what a stranger's
move, turn, resize, shape, flag, material, click action, name, sale, mask,
duplicate, undo, take, delete and return are answered with, with no right
over the object and with the move and the copy bits set for everyone, and
what a copy a stranger makes says of its owner and its masks. The case's
`MEASURED` table is the specification: this task is done when
`object-edit-stranger` runs on `FakeSl` and `FakeOpensim` and is in the
offline list.

Two things to carry over as measured rather than as one would guess: on
OpenSim a stranger's `Undo` **works**, and a refused turn, resize, shape
and flag are answered with nothing at all; on Second Life a stranger's
delete to the Trash and its `ObjectDelete` are refused in silence while a
take and a return are refused with `NoPermsNoRemoval`.

The deed leg needs groups: the fake grid has none, so today every group
set is a no-op and every deed is refused, which is the measured answer for
a group the agent is not in. A member's group set and a deed by a member
with the power wait on the fake grid's groups ([[gridspec-groups]]).

## Linking (2026-10-11)

[[gridspec-object-link-delink]] added `object-link-stranger` (two avatars,
live only): a stranger's link of an owner's two prims, a link of a prim of
each of two owners from either side and either way round, and a stranger's
delink of an owner's set. Written up in `book/src/gridspec/building.md`
§ Linking § Somebody else's prims; its `MEASURED` table joins
`object-edit-stranger`'s as this task's specification.
