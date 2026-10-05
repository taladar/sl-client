---
id: gridspec-parcel-info-dwell
title: Parcel info, dwell and the remote parcel id on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-parcel-info-reply-flags-misread,
  gridspec-search-directory, protocol-cap-product-info,
  server-fake-grid-directory-search, gridspec-land-transactions]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Green on OpenSim (dwell 0); OpenSim's flags byte packing known from source; SL's
unmeasured.

## Discover

`parcel-info-dwell` on aditi recording the flags byte of a for-sale and a
group-owned parcel.

## Document

`book/src/gridspec/land.md` § Parcel info.

## Fake grid

Small — the flags byte per flavour.

## Viewer

The decoded flags (see the bug).

## Done (2026-10-05)

- **Discover.** `parcel-info-dwell` now holds the listing to the parcel's own
  `ParcelProperties`, reads the listings of the first rows of a land search and
  a places search, compares each one's rating bits with its region's rating on
  the world map, and — where the agent may sell the parcel — puts it up for
  sale, reads the listing and takes it off again. Run on aditi (an adult
  sandbox whose centre parcel a group owns; 8 for-sale rows), on OpenSim as
  `estate-owner`, and on both fake flavours.
- **Measured** (`book/src/gridspec/land.md` § Parcel info). The flags byte
  packs rating, group ownership (`0x04`) and for-sale (`0x80`) alike on both
  grids, except that Second Life sets the moderate bit beside the adult one
  (`0x03`) and OpenSim does not (`0x02`). `SalePrice` is filled on parcels that
  are not for sale. Aditi's dwell is 0 everywhere and its `BillableArea` is 0
  on most listings. OpenSim serves a listing from a cache that lives 30 s past
  its last read.
- **Client.** `ParcelDetails::flags` is a `ParcelListingFlags`, the price is
  gated on its for-sale bit, and the encoder derives that bit from the price
  ([[protocol-parcel-info-reply-flags-misread]]). Second Life's land search
  arrives as a `DirLandReply` **event-queue** event, which the session dropped
  as unknown; it is decoded into the existing event.
- **Fake grid.** The listing's flags are derived from the region's rating, the
  parcel's owner and its sale state, with the adult packing per flavour
  (`ParcelPolicy::adult_listing_bits`).
- **Viewer.** About Landmark reads the typed flags; an `e2e` test opens it on a
  group's parcel in an adult region against each fake flavour.

## Beyond the task as written

- **OpenSim was measured too** (the task named aditi only): the for-sale bit
  live, as the estate owner. Its group-owned bit and its adult packing are read
  from source — deeding the local parcel to a group belongs to
  [[gridspec-land-transactions]], and the local grid has no adult region.
- **OpenSim's listing cache is not imitated.** It would cost every offline run
  of the case a minute and a half of waiting for a lag no viewer can act on;
  both fake flavours answer from the live record, and the case expects the lag
  on the live OpenSim only.
- **The land-search event's `ProductSKU` is not carried** — that is
  [[protocol-cap-product-info]], inside [[gridspec-search-directory]]. Nor does
  the fake grid answer a land search ([[server-fake-grid-directory-search]]),
  so the search half of the case runs on aditi only.
