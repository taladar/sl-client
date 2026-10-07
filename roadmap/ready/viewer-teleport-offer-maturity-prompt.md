---
id: viewer-teleport-offer-maturity-prompt
title: Ask before accepting a teleport offer rated above the maturity preference
topic: viewer
status: ready
origin: gridspec-teleport-lures (2026-10-07)
refs: [gridspec-teleport-lures, viewer-region-entry-maturity-gate,
  viewer-dialog-offers-invites]
---

Context: [context/viewer.md](../context/viewer.md).

A Second Life teleport offer states its destination's rating
(`InstantMessage::lure_destination`, measured in
`book/src/gridspec/teleport.md`, *Offers and requests*), and the offer card
shows it. Pressing Teleport on an offer rated above the user's preference
sends the acceptance regardless, and the grid refuses it: a `TeleportFailed`
with the alert `RegionTPAccessBlocked`, shown by the teleport display like
any other refusal. The user learns after the fact, and has to find the
preference themselves.

The reference viewer decides before the button exists
(`llimprocessing.cpp`, `IM_LURE_USER`):

- the rating is within the preference: the ordinary `TeleportOffered` card;
- the rating is above the preference and the account may raise it:
  `TeleportOffered_MaturityExceeded`, whose accept button changes the
  preference to the offer's rating and then accepts the lure
  (`mature_lure_callback`);
- the account may not see that rating at all (a teen account and Moderate, an
  unverified one and Adult): `TeleportOffered_MaturityBlocked`, which has no
  accept button, and the viewer declines the lure and sends the offerer the
  `TeleportMaturityExceeded` line itself.

All three templates are already in the notification catalogue
(`sl-viewer-notifications`, `catalogue/teleport.rs`) and nothing raises the
second or the third.

Wanted:

- The card chosen by the offer's rating against the stored preference and the
  account's ceiling, as the reference chooses it. An offer that states no
  rating (every OpenSim offer) gets the ordinary card.
- The change-and-continue button setting the preference through the existing
  maturity conversation of `sl-viewer-preferences` and accepting the lure
  once the grid has echoed it — a lure sent ahead of the echo is refused, and
  the refusal may spend the lure (not measured).
- The blocked card's automatic decline and its line to the offerer.
- An `e2e` case on the Second Life fake flavour: an agent set to General, an
  offer rated Moderate, the change accepted, the teleport carried out.

Shares its ceiling question with [[viewer-region-entry-maturity-gate]]: the
fake grid states `agent_access_max = "A"` for every account, so the blocked
card cannot be provoked there until that is settled.
