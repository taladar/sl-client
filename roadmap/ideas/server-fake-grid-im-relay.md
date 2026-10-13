---
id: server-fake-grid-im-relay
title: Fake grid — relay IMs and offers between its sessions
topic: server
status: ideas
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-sweep-two-avatars, server-message-routing]
blocked_by: [gridspec-teleport-lures, gridspec-instant-messages]
---

Context: [context/testing.md](../context/testing.md).

An `ImprovedInstantMessage` a viewer sends reaches its `SimSession` as a
`ServerEvent::InstantMessage` and goes no further: the fake grid does not
deliver it to the recipient's session. So two stage viewers cannot IM each
other, offer each other items, friendship or a teleport, or see each other
typing, and a test that needs the other side scripts it through
`FakeAgent::with_sim` instead (the do-not-disturb test does). The relay is
`SimSession::send_instant_message` on the recipient's root session, found by
agent id — the one-grid, one-process subset of
[[server-message-routing]], without offline storage or cross-host routing.

## From gridspec-teleport-lures (2026-10-07)

What a relayed teleport offer, decline and request must look like per
flavour is measured (`book/src/gridspec/teleport.md`, *Offers and requests*):

- **The offer** is made from the `StartLure` the offerer's session receives
  (`SimSession` forwards it raw today; it wants decoding), one
  `IM_LURE_USER` per target. Second Life's: an opaque lure id minted per
  offer, the bucket `gx|gy|x|y|z|lx|ly|lz|rating` with a padding space and a
  terminator, the offerer's `RegionID`, position zero, estate 1, no
  timestamp. OpenSim's: the
  id is `FakeParcelId` of the offerer's region and position (`z` plus two),
  an empty bucket, the offerer's position and region, a timestamp.
- **A lure store on the Second Life flavour.** A lure is spent by its first
  acceptance and by a decline, and a spent, declined or unknown lure is
  answered with nothing (`TeleportPolicy::unknown_lure` already is). It
  outlives its offerer's logout and lands the accepter where the offerer
  stood. The store replaces today's reading of an opaque id as the offerer's
  agent id. The OpenSim flavour needs none: its lure is a place for ever.
- **`IM_LURE_DECLINED` goes nowhere** on either flavour; on Second Life's it
  spends the lure.
- **`IM_TELEPORT_REQUEST` is relayed** on both. OpenSim's flavour gives a nil
  id the exclusive-or of the two agent ids, states the requester's region and
  estate 1 and a timestamp; Second Life's leaves the id nil, the region
  absent and the estate 0, and sends a one-byte bucket.
- **Nothing is stored for an avatar that is offline**, and the offerer is told
  nothing.
- The five two-avatar cases (`teleport-offer-accept`, `-decline`,
  `teleport-request`, `teleport-lure-offline`, and `-rated` on the Second
  Life flavour) join the offline list when the relay exists; each already
  states both grids' answers as `Measured`.

## From gridspec-instant-messages (2026-10-13)

What a relayed instant message must look like per flavour is measured
(`book/src/gridspec/chat.md`, *Instant messages*; the case
`instant-messages` already states both grids' answers and joins the offline
list when the relay exists):

- **What is rewritten.** Second Life's flavour states the sender's name,
  the session id of the two, the sender's region, estate 1, where the
  sender stands, a bucket of one zero byte and no time — whatever the viewer
  wrote, except a time, which it passes on. OpenSim's passes the viewer's
  name, id, place and bucket on, states the region, estate 1 and the time
  now, and an empty bucket stays empty.
- **Dialogs.** 0, typing and 20 are relayed on both; 1 and unknown ones are
  dropped on both; 19 from a viewer is dropped on Second Life's and relayed
  on OpenSim's. An automatic reply on Second Life's states no region, place
  zero and estate 0.
- **Text** is cut at 1023 bytes on both.
- **No mute is enforced** on either.
- **A message to oneself** arrives with the avatar's own id as the id.
- **Throttle (Second Life's only):** fourteen messages in a second, then a
  notice from no id signed "Second Life".
- **Away.** An ordinary message is kept and the sender told — from the
  recipient's id, signed "Second Life" / "System", texts as in the book. An
  unknown id is answered the same. Typing is dropped. An automatic reply is
  kept on Second Life's and refused on OpenSim's (dialog 20 from the
  sender's own id, signed "System"). A script's message is kept, its owner
  told nothing.
- **Reading back.** Nothing unasked. `RetrieveInstantMessages` on both, in
  the order sent, flagged kept, with the time sent. `ReadOfflineMsgs` on
  Second Life's only: an array holding one array of records, `transaction-id`
  nil, the id under `asset_id`, `message_time`, `estate_id`, `offline` 1 —
  `offline_messages_to_llsd` writes a bare array with the id as
  `transaction-id` today. Either road empties the store for both.
