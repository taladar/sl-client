---
id: gridspec-friends-presence
title: Friendship, rights, presence notifications and calling cards on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-friends-list-shows-a-sixth-rights-column,
  viewer-give-calling-card]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-14)

Measured and written up in `book/src/gridspec/friends.md`.

- **Discover.** One new two-avatar case, `friends-presence`: an end without
  a friendship; an offer declined, one accepted into B's calling-card folder
  and one to a friend; rights granted all, none and see-online, and B's map
  right in use (`FindAgent`, `TrackAgent` and the coarse `Prey`); a calling
  card accepted and one declined; and four absences of B — a logout, a
  vanishing without logout (`Session::vanish`, new), and kept offers
  answered over UDP, through the client's capability path, and declined
  through `DeclineFriendship` after a raw `ReadOfflineMsgs` read. Held to 76
  answers; nothing changed between two runs on either grid once the grant
  legs waited for Second Life's echo.
- **Findings.** Second Life tells the offerer nothing of a decline, passes an
  offer to an existing friend on, makes **no calling card** when a
  friendship forms, masks see-online out of every `ChangeUserRights` the
  grantee is sent (not out of the login buddy list), sets `Prey` once the
  map right is granted, and counts a vanished avatar offline after ~100 s.
  OpenSim tells the decline, refuses an offer to a friend with an alert,
  makes a card for each side, creates the card at *offer* time under its
  item id as the transaction, and hands a kept offer over unasked at login
  with its own text and A's id. Both capabilities take a kept offer however
  it was read; the decline answers `200 {success: true}`.
- **Viewer.** `Command::AcceptFriendship` / `DeclineFriendship` carry
  `use_offline_cap` (both runtimes: POST / DELETE through the capability with
  `friendship_answer_url`, a refusal logged, UDP where it is not granted);
  the viewer sets it for a kept offer, as the reference does. The Friends
  list drops the received see-online column
  ([[viewer-friends-list-shows-a-sixth-rights-column]]). No wait on a
  calling-card confirmation exists to remove.
- **Fake grid.** Large — [[server-fake-grid-friends-presence]], now ready.

### Differences from the plan

- The viewer bug [[viewer-friends-list-shows-a-sixth-rights-column]] was
  fixed here, as this task's "masked bit" viewer part.
- The edit right in use (editing a friend's object) was not measured.

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
