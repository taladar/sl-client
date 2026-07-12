---
id: protocol-35
title: ParcelProperties full field surface (extends #13, Tier B). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**35. `ParcelProperties` full field surface (extends #13, Tier B). ✅ Done.**
`ParcelInfo` (`sl-proto/src/types.rs`) previously carried only ~16 of the ~50
`ParcelData` fields. Added the full surface and populated it in *both* decode
paths — the UDP `parcel_info` (now taking the whole `ParcelProperties` message
so it can read the three trailing single-blocks) and the CAPS
`parcel_info_from_llsd`. Recovered fields: **`Name`/`Desc`** (decoded and thrown
away before), `GroupID` + `IsGroupOwned` (a group-owned parcel can now be told
from `owner_id`), `SalePrice`/`AuthBuyerID`/`AuctionID`/`ClaimDate`/
`ClaimPrice`/`RentPrice`/`PassPrice`/`PassHours`, the full prim accounting
(`OwnerPrims`/`GroupPrims`/`OtherPrims`/`SelectedPrims`/`TotalPrims`/
`ParcelPrimBonus`), avatar counts (`SelfCount`/`OtherCount`/`PublicCount`),
`Status`/`Category`/`LandingType` (as typed `ParcelStatus`/`ParcelCategory`/
`LandingType` enums), `SnapshotID`, `UserLocation`/`UserLookAt` (the teleport
landing point), and the region access/environment booleans
(`RegionDenyAnonymous`/`…Identified`/`…Transacted`/`…AgeUnverified`, push
override, `SeeAVs`/`AnyAVSounds`/`GroupAVSounds` as `Option<bool>` since the UDP
form omits them, and the `ParcelEnvironmentBlock`). **`RequestResult`** is now a
typed `ParcelRequestResult` (`NoData`/`Single`/`Multiple`) with a `has_data()`
helper, so a "no access / not found" reply is no longer silently surfaced as a
normal parcel. `ClaimDate` is read tolerantly — an integer `time_t` (SL/UDP) or
an ISO-8601 `date` (OpenSim CAPS), via a small clippy-clean
`parse_iso8601_to_unix` + `days_from_civil` helper. The three new enums and the
extended struct are re-exported through both runtimes, and `sl-survey`'s
`ParcelRecord` JSON now carries the parcel name/description, owner,
group-ownership, sale price and prim total. Covered by `sl-proto` lifecycle
tests (full UDP field surface, the `NoData` result, and the full CAPS LLSD form
incl. the ISO-date `ClaimDate` and the per-parcel AV-sound booleans).
*Live-verified against the local OpenSim via the `survey_probe` example: a
whole-region `ParcelProperties` over the CAPS event queue decoded `name="Your
Parcel"`, `request_result=Single`, owner id, `claim_date` (ISO date parsed to a
Unix `time_t`), `status=Leased`, `total_prims`/`other_prims`,
`parcel_prim_bonus=1.0`, `region_allow_access_override=true`,
`parcel_environment_version=-1`, and the three `Some(true)` AV-sound booleans
(`see_avs`/`any_av_sounds`/`group_av_sounds`) — all previously dropped. Test:
local OpenSim (both the UDP and CAPS parcel paths).*
