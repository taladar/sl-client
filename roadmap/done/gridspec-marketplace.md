---
id: gridspec-marketplace
title: Marketplace direct delivery on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-inventory-marketplace-operations]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Aditi grants `DirectDelivery` to a storeless avatar (404 merchant / listings);
OpenSim has none; the fake grid serves nothing.

## Discover

`marketplace-direct-delivery`; mutation routes need a merchant store (the user's
hands).

## Document

`book/src/gridspec/economy.md` § Marketplace.

## Fake grid

Small — an SL-flavour stub answering the measured 404s.

## Viewer

Marketplace UI hidden without the cap.

## Outcome (2026-10-19)

Discovered with the `marketplace-direct-delivery` census (now on all four
grids and in the offline list), which asks each route a storeless avatar can
ask raw — status, media type, redirect, body — and then through the client's
commands. Documented in `book/src/gridspec/economy.md` § Marketplace and
linked from `book/src/comms/marketplace.md`. Findings:

- Second Life grants `DirectDelivery` to a storeless avatar, and its service
  answers `GET /merchant`, `GET /listings`, `GET /listing/1`,
  `POST /listings` and a route it does not have alike: `404`,
  `text/plain; charset=utf-8`, `Not found`, no redirect. Nothing behind the
  capability tells a non-merchant's refusal from a missing route.
- OpenSim grants no `DirectDelivery`.
- The merchant-side routes (`PUT /listing`, `PUT /associate_inventory`,
  `DELETE /listing`) and what a merchant is answered stay unmeasured: no test
  account has a store.

Fake grid: small — `SimCaps` now serves `DirectDelivery` (a `CapHandler`
answering every route as aditi does), so the Second Life flavour grants it
and the OpenSim flavour withholds it (already in
`OPENSIM_REFUSED_CAPABILITIES`, now with the reason).

Viewer: the client reads Second Life's probe as "not a merchant" and its
listings as an SLM error, and both commands as a connection failure without
the capability; the census holds both readings on all four grids. The viewer
has no marketplace interface yet, so nothing needs hiding without the
capability; [[viewer-inventory-marketplace-operations]] is given the
findings.
