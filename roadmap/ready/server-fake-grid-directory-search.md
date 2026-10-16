---
id: server-fake-grid-directory-search
title: Fake grid — answer directory searches from an index of accounts, parcels and fixtures
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-search-directory]
---

Context: [context/server.md](../context/server.md).

## What

The protocol exists but the fake grid answers no `Dir*` query. Build the index;
OpenSim's people / groups-only answers and SL's sentinel as flavour rows.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured since (2026-10-05)

Second Life's land-search answer is the event-queue `DirLandReply`, not the UDP
message ([[gridspec-parcel-info-dwell]], `book/src/gridspec/land.md`), so the
Second Life flavour has to send it that way. `parcel-info-dwell` reads the
listings of a land search's rows wherever the search is answered, and will
start exercising the fake grid's index as soon as there is one.

## Measured (2026-10-16, [[gridspec-search-directory]])

`book/src/gridspec/search.md` has each grid's answer to every search, and the
`search-directory` census holds them (live grids only until this task: add it
to the offline list for both flavours when the index answers). In short:

- **Second Life**: everything over UDP but land search and land holdings,
  which go over the event queue (`DirLandReply`, `PlacesReply` with a
  `ProductSKU` per row). Pages of 101 rows in several messages; page two
  shares one row with page one (the marker). An empty or two-letter people
  text is not answered; a short places text is one nil row with status
  `SHORTSTRING`. Every people row says offline. Events and classifieds: one
  nil row / an empty reply with status `FOUNDNONE` on aditi. The UDP avatar
  picker answers one nil row, always; land holdings of an avatar with none,
  one nil row. `C_NONE` and `C_ANY` are different places searches.
- **OpenSim**: people and groups over UDP from the account and group
  services (a cut at 101, `online` always false, an empty text answered
  twice); places, land, events, classifieds and land holdings not answered at
  all.

`ProductInfoRequest` is already served per flavour.
