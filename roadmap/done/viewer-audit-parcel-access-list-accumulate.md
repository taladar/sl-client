---
id: viewer-audit-parcel-access-list-accumulate
title: Editing a multi-packet parcel ban list unbans everyone not in the last packet
topic: viewer
status: done
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

## Fixed (2026-08-27)

`merge_access_reply` folds each `ParcelAccessListReply` packet into the
accumulated list by id — updating an existing entry in place, mirroring the
reference's map insert — and returns whether anything changed, so the table
view rebuilds only when it did. The accumulator is emptied where the list is
*requested*, in `request_tab_data` via the new
`AboutLandState::clear_access_lists`, so a second request cannot inherit
entries the grid has since dropped.

Five unit tests in `about_land.rs` (previously zero for that file): successive
packets union, a repeated id updates in place, an identical packet reports no
change, an empty packet leaves the list alone, and clearing empties both lists
and bumps both revisions.

**Residual, deliberately not addressed:** the wire says nothing about how many
packets answer one request, so clicking Ban before the last packet lands still
uploads a partial list. The reference has the same shape and no mechanism to
close it; the window is the milliseconds between request and reply, against a
user who has to open the floater, switch tab and pick a resident.
