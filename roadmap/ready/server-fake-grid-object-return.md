---
id: server-fake-grid-object-return
title: Fake grid — return somebody else's object, auto-return, and end a temporary prim
topic: server
status: ready
origin: gridspec-object-rez-derez (2026-10-10)
refs: [gridspec-object-rez-derez, server-fake-grid-parcel-access-enforcement,
  gridspec-aditi-test-land]
---

Context: [context/server.md](../context/server.md).

[[gridspec-object-rez-derez]] measured three ways an object leaves a region
without its owner taking it (`book/src/gridspec/building.md`), and the fake
grid does none of them. Each needs something it has not got: an inventory
that is not the answering session's, or a clock.

- **A return of somebody else's object** (`DRD_RETURN_TO_OWNER` from the
  land's owner). OpenSim sends the returner the kill and nothing else; the
  object's owner is sent the kill, an item in its Lost And Found by the
  legacy announcement, and an instant message from `Server`. The fake grid
  kills the object and files nothing: a derez is answered under the
  answering session's lock, and the owner's inventory is another session's —
  or nobody's, when the owner is not logged in.
- **Auto-return** (`ParcelSetOtherCleanTime`). OpenSim returned a cube 62 to
  73 s after it was rezzed on a parcel set to one minute, with the same three
  things and "due to parcel autoreturn". The fake grid stores the minutes
  and does nothing with them.
- **A temporary prim.** Both grids take one away a minute or a little more
  after it is flagged — Second Life each in its own time (60.3 to 66.1 s),
  OpenSim in a sweep every 180 frames (60.8 to 76.1 s) — with a kill and
  nothing else. The fake grid sets the flag and the prim stands.

And one thing about the notice a return is followed by, which the fake grid
does send for an agent's own object: OpenSim sends it with the region's
next backup (every 200 frames) and puts every object of an agent's returned
since the last into one message ("Your 3 objects were returned from …").
The fake grid sends one at once for each.

## To do

- A per-region clock the grid's runtime drives — the sessions' timers are
  each their own — with a lifetime per flavour a test can shorten, since a
  minute is too long for `cargo test`. `object-rez-derez`'s temporary leg
  and `object-rez-land`'s two owner's legs then run offline on both
  flavours, held to what was measured.
- A way to file an item into the inventory of an agent that is not the
  answering session's, and to announce it there if it is logged in.
- `RezPolicy` rows (`sl-fake-grid/src/imitates.rs`) for what differs: when
  the notice comes and how it counts, whether temporary prims go one by one
  or in a sweep.
- Second Life's side of a return by somebody else and of an auto-return is
  not measured: we hold no land there ([[gridspec-aditi-test-land]]).
