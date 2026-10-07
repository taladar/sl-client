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
