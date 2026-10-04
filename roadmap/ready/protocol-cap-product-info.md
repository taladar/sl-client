---
id: protocol-cap-product-info
title: Land type names over ProductInfoRequest
topic: protocol
status: ready
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, gridspec-region-arrival]
---

Context: [context/protocol.md](../context/protocol.md).

Second Life grants `ProductInfoRequest`: GET → `[{sku, name, description}]`,
fetched once at login. Firestorm maps a `ProductSKU` to a land-type name in
land search, land holdings and group land. The SKU only arrives in the LLSD
forms of `DirLandReply` / `PlacesReply` (the UDP template comments it out), so
it also needs the event-queue forms decoded with their SKU.

Our land search shows "Auction" / "Sale" in that column. Fetch the list, carry
the SKU in `DirLandResult` / `PlacesResult`, and show the land type.

Every capability adopted here goes into `REQUESTED_CAPABILITIES`
(`sl-proto/src/session.rs`), is used where the region grants it, keeps the
existing path where it does not, and is served by the fake grid per flavour as
`seed-capabilities` measured it (`book/src/gridspec/capabilities.md`). Shapes
and Firestorm references: `book/src/comms/caps-reference.md`.
