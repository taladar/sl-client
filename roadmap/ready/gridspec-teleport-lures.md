---
id: gridspec-teleport-lures
title: Teleport offers, requests and their answers on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-message-routing]
---

Context: [context/gridspec.md](../context/gridspec.md).

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
