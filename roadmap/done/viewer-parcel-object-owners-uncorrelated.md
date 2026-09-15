---
id: viewer-parcel-object-owners-uncorrelated
title: An object-owner tally says nothing about which parcel it counted
topic: viewer
status: done
origin: found keying the About Land floater per parcel
  ([[viewer-keyed-floater-audit]], 2026-09-07)
refs: [viewer-keyed-floater-audit, viewer-remote-parcel-id-uncorrelated,
  viewer-parcel-join-split,
  viewer-parcel-options-general]
---

Context: [context/viewer.md](../context/viewer.md).

`ParcelObjectOwnersRequest` names the parcel it asks about. The reply does not:

```text
ParcelObjectOwnersReply Low 57 Trusted Zerocoded
{
    Data Variable
    {   OwnerID         LLUUID  }
    {   IsGroupOwned    BOOL    }
    {   Count           S32     }
    {   OnlineStatus    BOOL    }
}
```

No parcel id, no local id, no sequence id — just owners and counts. The same
shape as [[viewer-remote-parcel-id-uncorrelated]], and with the same
consequence once a floater can open on more than one subject: two About Land
windows asking at once cannot tell whose tally arrived, and the Objects tab
would show a neighbour's prim counts as this parcel's.

## What the viewer does today

`about_land.rs` serialises the requests. `OwnerTallyQueue` holds the windows
that want a tally, one request is outstanding at a time, and every reply while
it is belongs to the window that asked — which is also what keeps a tally split
over several packets whole. A window that goes unanswered for
`OWNER_TALLY_TIMEOUT_SECONDS` (or closes) releases its turn.
`owner_tallies_are_serialised` and `a_closed_window_releases_the_tally_turn`
pin both halves.

Correct, but it costs: a second window's Objects tab waits for the first, and
one silent request stalls the queue for the timeout.

## The fix

This one cannot be fixed client-side — the bytes carry no room for an answer.
Two ways forward, neither cheap:

- **Ask the grid**: `ParcelObjectOwnersReply` is `UDPDeprecated`, so the modern
  path for this data would be a capability, where a per-request POST answers its
  own question. Worth checking whether one already exists on Second Life before
  building around the UDP message.
- **Until then**, keep the queue and make the wait visible: the Objects tab of a
  window whose turn has not come should say it is waiting rather than showing an
  empty tally that looks like "no objects".

The reference never had to solve this: `LLFloaterLand` is a singleton on the
parcel the agent stands in, so it can only ever have one question outstanding.
Keying the window is what makes the gap real.

## How to verify

Open About Land on two parcels with different object owners: each Objects tab
must show its own parcel's tally, and neither may show the other's — including
after pressing **Refresh** in both.

## Note (2026-09-11)

[[viewer-remote-parcel-id-uncorrelated]] is fixed, and **this is not fixed with
it**. That one was a capability: a per-request POST, so the runtime holds the
question across it and stamps it into the answer. This one is UDP, and a
`ParcelObjectOwnersReply` has nowhere to put a question — so the queue and its
one-at-a-time rule stay until there is a capability to ask instead, and the
"make the wait visible" half above is the cheap part that can be done first.

## What was done (2026-09-15)

The protocol gap is still there — the reply names no parcel — but reading the
path end to end turned up defects that were ours, and fixing them narrows the
gap to what the protocol really leaves:

- **The event-queue form was never decoded.** `ParcelObjectOwnersReply` is
  `UDPDeprecated`, and the reference reads a `DataExtended` block the UDP
  template does not have — only the LLSD form carries one. A region with a
  queue (Second Life) answers there, and `handle_caps_event` had no arm for
  it, so the Objects tab could never fill on SL. Decoded now, with each
  owner's most recent rez time (`ParcelObjectOwner::most_recent`, a new
  **Most recent** column); `SimSession::enqueue_parcel_object_owners_reply`
  is the server side, and `sl-fake-grid` answers the request from its scene.
- **A tally split over packets kept only the last packet.** The ingest
  replaced the list on every reply while the queue's doc claimed it kept a
  split tally whole. Rows are folded in now (by owner, so a repeated packet
  counts nobody twice), and the list is emptied when the request goes out.
- **The reply carries its circuit** (`Event::ParcelObjectOwners::circuit`),
  so the queue keeps one question outstanding **per circuit**: windows on two
  regions ask at once, and a neighbour's reply never lands in this window.
- **The reply says whether it is whole** (`ParcelObjectOwnersPart`): the
  event-queue form is one document and ends the turn at once; a packet cannot
  say it is the last, so a UDP answer still holds the turn to the 8 s
  deadline (a straggler resent after a loss would otherwise be handed to the
  next window).
- **The wait is visible**: a status line beside Refresh says *Waiting for
  another request on this region…*, *Searching…*, *No objects on this
  parcel.* (only for an answer with nobody in it) or *The region sent no
  object list.*
- Nil-owner placeholder rows are dropped (the reference skips them), and a
  group owner's name is requested.

What remains is the protocol's: two windows on parcels of **one** region
still take turns, and on a grid answering by packet each turn is the full
deadline.

## Verified

Unit tests pin each rule (per-circuit turns, split tallies, whole replies
ending a turn, the unanswered and waiting lines, a refresh starting from
nothing); the event-queue form round-trips through `SimSession` and the fake
grid answers it end to end. Live on the local OpenSim (2026-09-15) the Objects
tab fills, Refresh re-asks and the status line follows. The **two-window**
case could not be tried live: the test region has one parcel, and splitting
one needs [[viewer-parcel-join-split]]. The Second Life (event-queue) path is
covered by tests only.
