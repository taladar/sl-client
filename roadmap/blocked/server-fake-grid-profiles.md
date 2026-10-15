---
id: server-fake-grid-profiles
title: Fake grid — profiles, picks, classifieds and notes with each grid's quirks
topic: server
status: blocked
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-profiles, protocol-sim-profile-messages]
---

Context: [context/server.md](../context/server.md).

## What

Store and serve profiles; OpenSim's ignored queries and volunteered replies as
flavour rows.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## From the census (2026-10-15)

Hold the store to `book/src/gridspec/profiles.md` and make the `profiles`
census run on both fake flavours (add them to its `grids`). Second Life's
flavour also serves `AgentProfile` (GET of the whole profile with its
`undef` texts, string `partner_id`, no `online` for a stranger and no
`hide_age`; PUT of any keys, `200` with no body) and sets an uploaded
profile image in its field (`ServerEvent::ProfileImageUploaded` already
stores the image). Flavour rows: the volunteered replies above; a L$0
classified refused on Second Life with "Invalid price for listing,
classified not published." and L$50 charged; OpenSim's classified delete
failing with "/Error deleting classified"; a pick's `User` (the parcel's
owner on Second Life, its name on OpenSim) and position.
