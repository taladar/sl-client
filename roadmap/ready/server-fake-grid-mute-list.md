---
id: server-fake-grid-mute-list
title: Fake grid — mute list storage, the Xfer file and each grid's empty-list reply
topic: server
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-mute-list]
---

Context: [context/server.md](../context/server.md).

## What

Mute requests are dropped. Decode them server-side, store, serve the CRC-cached
file and `emptymutelist` — as measured by `mute-list` (2026-10-16,
`book/src/gridspec/friends.md` § Mute list):

- an empty list (any CRC) and a list emptied by removals answer
  `emptymutelist`; a list answers `MuteListUpdate` + the file over Xfer; a
  request naming the IEEE CRC-32 of the file answers `UseCachedMuteList`;
- the file is `<type> <id> <name>|<flags>` lines in the order added; Second
  Life writes a by-name entry's flags field **empty** (`…|`), OpenSim `|0`;
- Second Life keys an entry by id (a re-mute under another name renames it, a
  removal under any name removes it); OpenSim by id **and** name (a second
  name files a second entry, a removal needs the exact name);
- a self-mute is not kept on either grid; edits are never answered;
- **nothing is enforced** on either grid — every message a muted resident sends
  is delivered and the sender is told nothing.

Add `mute-list` to the offline list for `FakeSl` and `FakeOpensim` once the
store is in.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## From server-fake-grid-im-relay (2026-10-23)

`instant-messages` runs on both fake flavours with
`mute_list_after_the_mute` exempt (its `FAKE_EXEMPT`, naming this task):
take it out of that list once the store answers `MuteListRequest`.
