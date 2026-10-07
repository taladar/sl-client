---
id: gridspec-teleport-lures
title: Teleport offers, requests and their answers on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-message-routing, server-fake-grid-im-relay,
  viewer-teleport-offer-maturity-prompt, protocol-im-states-no-position,
  gridspec-instant-messages, viewer-region-entry-maturity-gate]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-07)

Measured and written up in `book/src/gridspec/teleport.md`, *Offers and
requests*.

- **Discover.** Six two-avatar conformance cases, five of them new, each
  recording the whole exchange: the offer or request as delivered (every
  field, the lure id's shape, the binary bucket), the teleport an acceptance
  starts (through the shared `teleport_trace`), and what the grid says to
  each avatar in the ten seconds after each step (a new `sl_conformance::lure`
  watch). `teleport-offer-accept` (rewritten: a local and a cross-region
  acceptance), `teleport-offer-decline`, `teleport-request`,
  `teleport-lure-unknown` (one avatar), `teleport-lure-offline` and
  `teleport-lure-rated` (aditi only). All ran on aditi and OpenSim.
- **Findings.** Second Life's lure id is opaque and its offer names the
  destination and its rating in the binary bucket; OpenSim's lure id *is* the
  place and its bucket is empty. Neither grid tells the offerer anything —
  not on accept, not on decline. On Second Life a lure is spent by its first
  use and by a decline, and a dead lure (spent, declined or never issued) is
  answered with **nothing at all**; OpenSim's can be used for ever and one it
  cannot read is refused as a region that does not exist. A lure outlives its
  offerer's logout on both. Second Life opens a lure with the progress line
  `completing`, local or not, and refuses one into a region above the
  accepter's maturity preference with a `TeleportFailed` alone. OpenSim's
  `TeleportFinish` says `VIA_LOCATION` whatever kind it finishes. A teleport
  request has no reply message on either grid; OpenSim rewrites its nil id.
  Neither grid stores an offer for an avatar that is offline.
- **Client.** The session's teleport deadline now records a
  `Diagnostic::ExpectedReplyMissing` for `Teleport`, so "the grid said
  nothing" can be told from a refusal. `InstantMessage::lure_destination`
  decodes Second Life's bucket (`LureDestination`, re-exported by both
  runtimes).
- **Fake grid.** Three rows of `ImitatedGrid::teleport_policy`: the answer to
  a lure it cannot resolve (silence, or OpenSim's sentence), the lure's
  opening line, and the finish's flags. A lure above the maturity preference
  is refused without a start on the Second Life flavour.
  `teleport-lure-unknown` runs offline on both flavours.
- **Viewer.** The offer card shows the destination's rating when the offer
  states one. A teleport *request* raises a card (it raised nothing before):
  Offer Teleport answers with an offer, Decline sends nothing.
  `e2e_two_avatars` holds the cards and each answer on both fake flavours,
  and a second, two-viewer test of it ran live on OpenSim and aditi: an offer
  from the radar, declined without a word to the offerer, and a second one
  taken.
- **Not done here.** Relaying offers, declines and requests between fake-grid
  sessions, with a lure store that spends a Second Life lure
  ([[server-fake-grid-im-relay]], which now carries the measured shape). The
  reference's question before a rated offer is accepted
  ([[viewer-teleport-offer-maturity-prompt]]).
  Not measurable or not measured: how long an unused Second Life lure lasts,
  whether a rating refusal spends it, an offer to several avatars, a godlike
  lure, and OpenSim's answer to a rated lure (its regions are all General).
  One client gap was found and filed: [[protocol-im-states-no-position]].

## Known already

OpenSim's lure id encodes handle and position, SL's is opaque (client fixed);
OpenSim sends the offerer nothing on accept. The fake grid resolves lures but
relays nothing between sessions.

## Discover

`teleport-offer-accept` on both grids plus decline and teleport-request
(`IM_TELEPORT_REQUEST`) legs; two avatars; the two-viewer `e2e_two_avatars`
live.

## Document

`book/src/gridspec/teleport.md` § Lures.

## Fake grid

Large — implemented with the IM relay, [[server-fake-grid-im-relay]].

## Viewer

Accept / decline feedback as each grid gives it.

## From gridspec-teleport (2026-10-07)

A lure whose host is gone is refused by the fake grid with `no_host` on both
flavours, over each flavour's transport; neither grid's own answer is
measured. It is a row of `ImitatedGrid::teleport_policy`.
