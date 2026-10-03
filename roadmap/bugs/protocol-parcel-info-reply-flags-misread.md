---
id: protocol-parcel-info-reply-flags-misread
title: ParcelInfoReply's packed flags byte is read as the parcel-flags field
topic: protocol
status: bugs
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`sl-proto/src/session/methods.rs` (the `ParcelInfoReply` arm) gates the sale
price on `data.flags & 0x04` with the comment "carries PARCEL_FOR_SALE
(0x04)". That is the full parcel-flags field's bit. The reply's `Flags` is a
separate packing (`llremoteparcelrequest.h`: `U8 flags; // group owned,
maturity`): 0x1 mature, 0x2 adult, 0x4 group-owned
(`llpanellandmarkinfo.cpp`). OpenSim (`LLClientView.cs` ~3724) puts maturity
in the low bits, `1 << 7` for for-sale, `0x04` for group-owned.

## Effect

A group-owned parcel reports a sale price; a parcel that is for sale (0x80)
loses its price. `ParcelDetails.flags: u8` carries the raw byte on, and
`SimSession::send_parcel_info_reply` writes it verbatim while
`sl-fake-grid/src/world.rs` sets `flags: 0` with a `sale_price` — so the fake
grid's own for-sale listings arrive priceless. `about_landmark.rs` decodes
the same byte by hand again (0x2 / 0x1 / 0x4).

## Fix

A `ParcelListingFlags` bitflags type (MATURE, ADULT, GROUP_OWNED,
FOR_SALE = 0x80) decoded once, or decode straight to `{ maturity, group_owned,
for_sale }`; the encoder derives the bits from the typed fields. Check what
Second Life actually sends for for-sale (aditi) before choosing the sale-price
rule — the reference viewer does not read a for-sale bit at all.
