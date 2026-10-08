---
id: gridspec-friends-presence
title: Friendship, rights, presence notifications and calling cards on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-friends-list-shows-a-sixth-rights-column,
  viewer-give-calling-card]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

SL masks `CAN_SEE_ONLINE` out of grantee views; OpenSim rejects an offer to
an existing friend and no-ops calling-card accept; the fake grid has a static
buddy list.

## Discover

The friendship / rights / presence / calling-card cases on both grids; decline
path; offer to an existing friend; calling card on accept; crash vs clean logout
notifications.

## Document

`book/src/gridspec/friends.md`.

## Fake grid

Large — [[server-fake-grid-friends-presence]].

## Viewer

The masked bit on SL; never wait for OpenSim's calling-card confirmation.

## Capabilities done in this task

[[protocol-cap-offline-friendship-answers]]: answering offline friendship
offers over `AcceptFriendship` / `DeclineFriendship`, measured together with
[[gridspec-instant-messages]]' offline delivery.

## From gridspec-avatar-presence (2026-10-08)

A `TrackAgent` for a resident who is not a friend never set the coarse
feed's `Prey` index on aditi (and OpenSim's encoder never sets it at all).
Whether Second Life sets it for a friend who granted map rights is this
task's: `avatar-presence` records `track_sets_prey`, so grant the right and
run it again.
