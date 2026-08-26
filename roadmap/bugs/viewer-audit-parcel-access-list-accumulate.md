---
id: viewer-audit-parcel-access-list-accumulate
title: Editing a multi-packet parcel ban list unbans everyone not in the last packet
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-places/src/about_land.rs:1555` and `:1559` ingest a
`ParcelAccessList` reply with `state.access_allow.clone_from(entries)` /
`state.access_ban.clone_from(entries)` — a **replace** per reply packet. A list
long enough to span packets therefore ends up holding only the last packet.

Then `send_access_list` (`:2648`) sends the whole local list as a
**replacement** (`Command::UpdateParcelAccessList`, documented at
`sl-proto/src/command.rs:951` as "Replace a parcel's allow or ban list") on
every add (`:2616`) and remove (`:2635`).

Net effect: banning one more person on a long ban list **silently deletes every
entry that was not in the last packet received**, on the server.

The reference accumulates without clearing — `LLParcel::unpackAccessEntries`
does `(*list)[entry.mID] = entry;` (`llparcel.cpp:684`) into the existing map.
Note the wire `SequenceID` is **not** the fix: `ParcelAccessListReply` carries
it (`message_template.msg:4934`) and `sl-proto/src/session/methods.rs:2918`
drops it, but the reference reads it and marks it `//ignored`
(`llviewerparcelmgr.cpp:2171`).

Scope: accumulate by id on ingest and clear when the parcel selection changes.
Extract `fn merge_access_reply(existing: &mut Vec<ParcelAccessEntry>, reply:
&[ParcelAccessEntry]) -> bool` so the union is a unit test — `about_land.rs` is
3309 lines with zero tests.
