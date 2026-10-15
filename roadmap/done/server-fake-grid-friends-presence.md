---
id: server-fake-grid-friends-presence
title: Fake grid — a mutable friendship store with rights, presence fan-out and calling cards
topic: server
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
blocked_by: [gridspec-friends-presence]
---

Context: [context/server.md](../context/server.md).

## What

Only a static buddy list exists. Offer / accept / decline / terminate, rights
grants with SL's masking, online / offline fan-out on session open and close,
calling cards.

## How

Implement the behaviour the discovery task measured and the book's
`gridspec` chapter records, per `ImitatedGrid` flavour (a row in
`sl-fake-grid/src/imitates.rs` for every divergence), and hold it to the
measurement with the discovery task's conformance cases run against
`FakeSl` and `FakeOpensim`. See `roadmap/context/gridspec.md`.

## Measured (gridspec-friends-presence, 2026-10-14)

`book/src/gridspec/friends.md`, held by `friends-presence` (add the fake
flavours to its grids once there is a store). Per flavour: who is told of a
decline, an offer to a friend, calling cards at accept and at offer, the
see-online mask in `ChangeUserRights`, `Prey` with the map right, the
vanish timeout, a kept offer handed over unasked (OpenSim) or kept (SL), and
the `AcceptFriendship` / `DeclineFriendship` capabilities on `FakeSl`.

Until `FakeSl` grants `AcceptFriendship` and `AcceptGroupInvite` (both
`Pending` in `sim_caps.rs`), the client's drain (`RequestOfflineMessages`,
[[protocol-cap-read-offline-msgs]]) reads its kept messages over the UDP
`RetrieveInstantMessages`, which the fake grid does not answer: grant them
with the store, and answer the UDP trigger for `FakeOpensim`.

## Done (2026-10-15)

`sl-fake-grid/src/social.rs`: a grid-wide store (friendships with the rights
each way, offers and calling cards pending, what is kept for an avatar that is
away, who is online) and a responder on every session, with a `FriendsPolicy`
row per divergence. Presence follows the agent's last root session, so a
logout and the circuit's timeout both take a friend offline. `SimSession`
gained the coarse feed's `Prey` (`set_coarse_prey`), and serves
`AcceptFriendship` / `DeclineFriendship` as a `KeptFriendshipAnswered` event.
Every agent now has stock folder ids of its own, so A's calling-card folder
is not B's.

A kept message is now a `KeptMessage`: what `RetrieveInstantMessages` hands
over and what the `ReadOfflineMsgs` record states, which Second Life words
differently. The record encoder writes Second Life's shape (an array holding
one array, a nil `transaction-id`, the id under `asset_id`, `offline`,
`message_time`, `estate_id`).

`friends-presence` holds both fake flavours offline (about 8 and 9 minutes a
flavour, most of it the case's own windows and the vanish timeout).

## Differences

- Only friendship offers and group invitations are kept for an avatar that
  is away; every other instant message still goes nowhere
  (`server-fake-grid-im-relay`).
