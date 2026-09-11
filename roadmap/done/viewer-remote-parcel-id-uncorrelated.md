---
id: viewer-remote-parcel-id-uncorrelated
title: A resolved parcel id names no question, so two askers cannot be told apart
topic: viewer
status: done
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

## Resolved (2026-09-11)

Done as filed, by the route the codebase had already taken once: the runtime
stamps the question into the answer. `AvatarPickerSearch` does exactly this with
its `query-id` — the HTTP path carries none of its own — so this is that trick
applied to a second bare-valued capability rather than a new mechanism.

`sl-wire` owns the format, since a codec belongs in the pure crate.
`stamp_remote_parcel_request` writes the request's own three keys (`location`,
`region_id`, `region_handle`) into the reply map, so
`parse_remote_parcel_request` reads the echo straight back — one vocabulary
whichever direction the question travels. Both region fields are written even
when only one was sent, so a handle-addressed request round-trips as a
handle-addressed request rather than as a different question.
`parse_remote_parcel_answer` returns the parcel id and the question together, as
a `RemoteParcelAnswer`.

`Event::RemoteParcelId` is now a struct variant carrying `parcel_id`,
`location`, `region_id` and `region_handle`. Both runtimes grew a
`post_remote_parcel_request` / `run_remote_parcel_request` that holds the
`RemoteParcelRequest` across the POST and stamps it on the way back, in place of
the generic `post_voice_cap` / `run_voice_cap` they used to borrow.

**An unstamped reply is an error, not a default.** `parse_remote_parcel_answer`
requires `location` and fails without it. The tempting alternative — default the
echo to the origin — is the bug wearing a different hat: every window waiting
would match a defaulted origin equally well, so the wrong parcel's name, owner
and traffic would land in a window just as plausibly as before. The grid never
sends these keys; only a runtime that forgot to stamp can produce one, and that
should be loud.

About Landmark lost `ParcelResolveQueue`, `drive_parcel_resolves`, and the
one-at-a-time rule. A window fires its own request the moment its asset parses
and recognises its answer by the region and position it asked about. The timeout
is now per window: an unanswered request stalls nothing but its own window,
where before it held the shared slot for the full ten seconds.

An answer reaches every window that asked that same question, and one
`ParcelInfoRequest` follows however many those are — two landmarks in one parcel
are one question answered once, and `ParcelDetails` already names its parcel, so
one reply fills them all.

Tests. In `sl-wire`: a stamped reply carries its question back; a
handle-addressed request round-trips; an unresolved (`{}`) answer is `None`, not
an error; an unstamped reply is an error. In `sl-proto`'s lifecycle suite the
event's echo is asserted against what was asked, and the same reply without the
stamp surfaces nothing; `sim_caps` folds through the stamp the way a runtime
does. In the viewer: both windows ask on the same frame; answering the *second*
landmark first fills the second window and leaves the first empty and still
resolvable (arrival order would have got this exactly backwards); and two
windows on one destination share the answer and one `ParcelInfoRequest`.

The conformance case `parcel_info_dwell` now waits for the reply to the location
it asked about rather than the first `RemoteParcelId` to arrive.

`ParcelObjectOwnersReply` — [[viewer-parcel-object-owners-uncorrelated]] — is
the same problem one layer down and is *not* fixed by this: its bytes have no
room for an echo, so it still needs a capability or a visible wait.
