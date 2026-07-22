---
id: protocol-slm-directdelivery
title: sl-marketplace crate — SLM DirectDelivery JSON transport
topic: protocol
status: ready
origin: split out of viewer-inventory-marketplace-operations
  (2026-07-22) — the SLM API is JSON over a cap URL, unlike every
  LLSD transport we speak, so it gets its own crate and book chapter
refs: [viewer-inventory-marketplace-operations]
---

Context: [context/protocol.md](../context/protocol.md).

The Second Life Marketplace (SLM) service behind the region's
**DirectDelivery** capability: a plain-JSON REST API (the only
non-LLSD transport in the protocol surface), with the routes the
reference viewer drives (`llmarketplacefunctions.cpp`) —
`GET /merchant` (merchant probe, distinct not-a-merchant /
not-migrated errors), `GET /listings` + `GET /listing/<id>`
(`id`, `is_listed`, `edit_url`,
`inventory_info { listing_folder_id, version_folder_id,
count_on_hand }`), `POST /listings` (create),
`PUT /listing/<id>` (list / unlist, switch version folder),
`PUT /associate_inventory/<id>`, `DELETE /listing/<id>` (archives).

One task, four pieces:

- **A new `sl-marketplace` crate**: sans-IO SLM API model — typed
  listing / merchant-status / error records, request builders and
  response parsers for every route (serde JSON, no sockets) — the
  transport is different enough from the LLSD CAPS stack that it does
  not belong in `sl-wire`.
- **Low-level integration**: retrieve the `DirectDelivery` cap with
  the rest of the seed caps; `Command`s / `Event`s in `sl-proto`
  (`MarketplaceMerchantStatus`, `MarketplaceListings`,
  `Marketplace{Create,Update,Associate,Delete}Listing`) executed
  through the new crate; wired through **both** runtime shells
  (`sl-client-tokio` / `sl-client-bevy`, the parity rule).
- **Book**: a new transport chapter under `comms/` describing the SLM
  JSON transport as a peer of LLUDP and the LLSD CAPS — cap
  discovery, JSON framing, the route table, error codes.
- **Live verification**: OpenSim has no DirectDelivery at all; on
  aditi only the transport level is reachable without a merchant
  store (`GET /merchant` answering with a proper non-merchant error,
  `GET /listings`) — record that as the conformance ceiling.
