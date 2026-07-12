---
id: api-g7
title: Parcel completeness
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G7 — Parcel completeness

`ParcelJoin`, `ParcelDivide`, `ParcelObjectOwnersRequest`/`Reply`,
`ParcelBuyPass`, `ParcelDisableObjects`, `ParcelInfoRequest`/`ParcelInfoReply`
(basic info by parcel id; modern path is the `RemoteParcelRequest` CAPS — add
both). OpenSim-testable.

- [x] G7 parcel join/divide/object-owners/pass/disable/info.
  New `ParcelObjectOwner` + `ParcelDetails` types (`types/parcel.rs`). Commands
  `JoinParcels`/`DivideParcel` (metre rectangles), `RequestParcelObjectOwners`
  (→ `Event::ParcelObjectOwners`), `BuyParcelPass`, `DisableParcelObjects`
  (owner/group/other or id-list scope, mirrors `ReturnParcelObjects`),
  `RequestParcelInfo` (by grid-wide parcel id → `Event::ParcelDetails`), and the
  CAPS `RequestRemoteParcelId` (`RemoteParcelRequest` POST →
  `Event::RemoteParcelId`, the modern id-resolution path;
  new `CAP_REMOTE_PARCEL_REQUEST` +
  `sl-wire/src/remote_parcel.rs` build/parse pair, location + region_id|handle).
  Circuit encoders + `Session` methods; server decodes each UDP message into a
  same-named `ServerEvent` plus `SimSession::send_parcel_object_owners_reply` /
  `send_parcel_info_reply`. Both runtimes (the remote-parcel POST reuses the
  generic voice-cap POST helper) + REPL (7 commands) + format.rs. Tests: 3
  lifecycle client (encode, object-owners decode, parcel-info decode) + 1 CAPS
  decode + 1 loopback round-trip + 3 wire codec + 4 REPL registry. Book:
  extended `content/world.md` Parcels section. **Scope note:** the
  `*Reply`/backend halves stay decode-only as before; `ParcelInfoReply`
  (Trusted) is wrapped as an `Event`/`SimSession` encoder (it is viewer-facing).
  Both UDP
  by-id and the CAPS resolution path implemented per the roadmap. NOT
  live-tested (loopback + wire round-trips cover both directions). **NEXT = G8**
  (estate covenant & telehub).
