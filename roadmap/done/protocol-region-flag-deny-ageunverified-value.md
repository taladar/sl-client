---
id: protocol-region-flag-deny-ageunverified-value
title: RegionFlags::DENY_AGEUNVERIFIED has the wrong bit; region and estate flags are split
topic: protocol
status: done
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns]
---

Context: [context/protocol.md](../context/protocol.md).

## Observation

`sl-wire/src/parcel_flags.rs` has `RegionFlags::DENY_AGEUNVERIFIED = 1 << 24`;
`sl-proto/src/types/map.rs` has `EstateFlags::DENY_AGEUNVERIFIED = 1 << 30`.
The reference (`llregionflags.h`) is `REGION_FLAGS_DENY_AGEUNVERIFIED =
1ULL << 30`. `EstateFlags`' doc says the estate flags "differ from"
`RegionFlags`, but both are the one `REGION_FLAGS_*` space.

## Effect

Nothing reads `RegionFlags::DENY_AGEUNVERIFIED` today, so this has not fired —
the first consumer would silently test bit 24.

## Fix

Fix the bit, then merge the two into one `RegionFlags` over `u64` (the
extended width the reference uses), retyping the raw `u32` / `u64` region-flag
fields in `region.rs` / `EstateInfo.estate_flags` (see
[[idiomatic-protocol-codes-past-decode]]). Cross-check every other constant of
both types against `llregionflags.h` while there.

## Done (2026-10-17)

Fixed in [[gridspec-region-info]]: `sl_wire::RegionFlags` is a `u64` with every
`REGION_FLAGS_*` of `llregionflags.h` (bit 30 for `DENY_AGEUNVERIFIED`),
`with`, `from_legacy`/`legacy_bits` and `ESTATE_MASK`; `EstateFlags` is gone
and `EstateInfo::estate_flags`, `EstateInfoUpdate::flags`,
`GodRegionUpdate::region_flags` and the region records' flag fields are typed.
`MapRegionInfo::region_flags` stays raw.
