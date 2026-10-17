---
id: server-fake-grid-estate-actions
title: Fake grid — kick, eject, freeze, teleport home, estate messages and restarts
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-experiences, gridspec-estate]
---

Context: [context/server.md](../context/server.md).

## What

None of the estate actions is answered. Remove agents, teleport them home,
deliver estate messages and restart notices per flavour.

Estate rights are not modelled at all yet, so the fake grid also applies a
`RegionExperiences` POST from anyone; Second Life answers a non-manager's
`405 Method Not Allowed` ([[gridspec-experiences]],
`book/src/gridspec/experiences.md`).

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured by gridspec-estate (2026-10-17)

`book/src/gridspec/estate.md` has OpenSim's answer to every action, as the
estate owner on a resident in the region; Second Life's only as a resident
without rights. Already in the fake grid: the region and estate messages, the
first restart notice (each flavour's form), the `EstateAccess` capability and
both lists after an allowed/banned change. Still to do, per flavour:

- freeze / unfreeze / a freeze wearing off after 30 s, and eject (with a ban)
  — the alerts to both avatars, B moved; Second Life's keyed refusals
  `AvatarFreezeFailure` / `AvatarEjectFailed` for a resident;
- `teleporthomeuser` / `teleporthomeallusers` (OpenSim sends the sender home
  too) and a ban sending its subject home, kicking one with no home;
- `kickestate` (`KickUser` "You have been kicked out", then the close);
- the restart countdown's later notices (OpenSim: every 2 min, then 30 s,
  then 15 s) and the restart itself ("The simulator is going down.");
- then add `Grid::FakeSl` / `Grid::FakeOpensim` to the `estate` census's
  grids and hold the fake flavours to it.
