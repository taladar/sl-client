---
id: viewer-parcel-object-owners-uncorrelated
title: An object-owner tally says nothing about which parcel it counted
topic: viewer
status: bugs
origin: found keying the About Land floater per parcel
  ([[viewer-keyed-floater-audit]], 2026-09-07)
refs: [viewer-keyed-floater-audit, viewer-remote-parcel-id-uncorrelated,
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
