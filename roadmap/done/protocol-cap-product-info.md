---
id: protocol-cap-product-info
title: Land type names over ProductInfoRequest
topic: protocol
status: done
origin: protocol-reference-capabilities triage (2026-10-04)
refs: [protocol-reference-capabilities, gridspec-region-arrival]
blocked_by: [gridspec-search-directory]
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

## Done together with [[gridspec-search-directory]]

This capability is adopted inside that gridspec task, which measures the
feature on both grids over both paths, makes the fake grid serve the
capability per flavour, and checks the viewer. Claim that task.

## Already in place (2026-10-05)

The event-queue `DirLandReply` is decoded (`dir_land_reply_from_caps_llsd`,
[[gridspec-parcel-info-dwell]]); its `ProductSKU` is read past, not carried.

## Done (2026-10-16, in [[gridspec-search-directory]])

`CAP_PRODUCT_INFO_REQUEST` is requested; `Command::RequestLandProducts` GETs
it in both runtimes into `Event::LandProducts` (a `LandProduct` per entry:
`sku`, `name`, `description`); the REPL has `request_land_products`.
`DirLandResult` and `PlacesResult` carry `product_sku` from the event-queue
forms (`None` over UDP), and the event-queue `PlacesReply` is decoded — it
was dropped before, so Second Life's land holdings never arrived. Aditi lists
26 products; every land-for-sale row's SKU is one of them. The fake grid's
Second Life flavour serves the 26 (`ImitatedGrid::land_products`), the
OpenSim flavour withholds the capability. The search floater asks for the list
once, before its first land search, and names a land row's type by its SKU
("Unknown" otherwise, as the reference).
