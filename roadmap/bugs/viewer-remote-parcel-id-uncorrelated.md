---
id: viewer-remote-parcel-id-uncorrelated
title: A resolved parcel id names no question, so two askers cannot be told apart
topic: viewer
status: bugs
origin: found keying the About Landmark floater per landmark
  ([[viewer-keyed-floater-audit]], 2026-09-07)
refs: [viewer-keyed-floater-audit, viewer-about-landmark-floater]
---

Context: [context/viewer.md](../context/viewer.md).

`RequestRemoteParcelId` POSTs a location to the `RemoteParcelRequest`
capability, and the answer comes back to the session as
`Event::RemoteParcelId(parcel_id)` — a bare id. Nothing in the reply says which
request it answers: not the location asked about, not the region id, not a
correlation tag.

That is invisible while one window at a time asks. It stopped being invisible
when About Landmark became a keyed floater: two landmarks open at once are two
independent resolves, and with two in flight the client cannot tell which id
belongs to which window. Assigning the wrong one is worse than not resolving —
the window would fill with a *different parcel's* name, owner, traffic and
snapshot, all of it plausible.

## What the viewer does today

`about_landmark.rs` serialises them: `ParcelResolveQueue` holds the windows that
have asked, exactly one request is in flight, and the reply belongs to the
window at the head. A window whose resolve deadline passes leaves the queue and
the next one's request goes out. `parcel_resolves_are_serialised` pins it, and
it was that test which found the second half of the same problem — one reply was
being offered to every window in turn, so the window *behind* the head (which
becomes the head the instant the first is answered) took the same answer as its
own. Now a `RemoteParcelId` is consumed once, by the head; `ParcelDetails`,
which does name its parcel, still reaches every window waiting on that parcel.

This is correct but slow: two landmarks resolve one after the other, and one
unanswered request stalls every window behind it until its deadline.

## The fix

The capability is a **per-request POST** — the client holds the request while
the response arrives — so the answer *can* carry its question. Thread the
request's location and region id (or a client-side request id) through
`sl-proto`'s caps machinery and put them on the event:

```text
Event::RemoteParcelId { parcel_id, location, region_id }
```

Then every waiting window matches the reply against what it asked, the queue and
its one-at-a-time rule go away, and resolves run concurrently. The same shape
would suit any other capability whose reply is currently a bare value.

## How to verify

Open two landmarks in different regions at once: both must fill with their own
parcel, and neither may wait on the other. A unit test can drive two windows'
resolves and answer the *second* one first — impossible to get right without the
correlation, which is the point.
