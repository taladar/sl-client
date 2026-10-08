---
id: gridspec-aditi-test-land
title: Decide how we get land and estate rights on aditi for the land, estate and terraform measurements
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, gridspec-parcel-management, gridspec-estate,
  gridspec-terrain-editing, gridspec-parcel-access-and-ban-lines,
  gridspec-land-transactions]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

On aditi our avatars own no parcel and hold no estate rights, so every
parcel-edit, access-list, divide/join, return, terraform and estate leg is
either unrun or `partial` there (OpenSim's are run as `estate-owner`).
Estate commands are refused silently without rights.

## Discover

**A decision for the user**, not an implementation task: buy or rent a small
parcel on aditi (beta L$), join a group that owns one, or ask Linden Lab for
a test estate / sandbox with terraform rights. Record what was obtained, its
region and parcel, in `fixtures.aditi.toml` (gitignored) and in the land
tasks.

## Document

Nothing beyond the land chapters' notes on which legs were measured where.

## Fake grid

Not applicable.

## Viewer

Not applicable.

## From gridspec-avatar-presence (2026-10-08)

`parcel-privacy` (a parcel with `SeeAVs` off: who is sent whom across its
line) ran on OpenSim only and waits on this for its Second Life column in
`book/src/gridspec/avatars.md`. It needs a parcel it can divide; add
`Grid::Aditi` to its grids and a fixture for where the parcel is.
