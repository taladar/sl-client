---
id: gridspec-inventory-offers
title: Giving inventory, accepting and declining, and object gives on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-message-routing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim files the copy at offer time and relays `IM_INVENTORY_ACCEPTED`; SL
assigns the copy id on accept and relays nothing to the giver.

## Discover

`give_inventory` plus decline, folder gives, offline recipients, object gives
(scripted prim), the SL inbox filing; the two-viewer live tests.

## Document

`book/src/gridspec/inventory.md` § Giving.

## Fake grid

Large — [[server-fake-grid-inventory-offers]].

## Viewer

No dependence on the bucket id; no wait for a giver ack on SL.
