---
id: idiomatic-p6-02
title: money::LindenAmount — extended to the non-negative L$ *price* fields (
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`money::LindenAmount` — extended to the non-negative L$ *price*
fields (maximal scope: every carrier of the named fields, not just the named
files). Converted: the `EconomyData` price block (`price_energy_unit`,
`price_object_claim`, `price_public_object_decay`/`_delete`,
`price_parcel_claim`, `price_upload`, `price_rent_light`,
`teleport_min_price`, `price_parcel_rent`, `price_group_create` — the `f32`
multipliers/exponents and the object counts stay raw); `ownership_cost` +
`sale_price` on
`ObjectProperties`/`ObjectPropertiesFamily`; `sale_price` on `InventoryItem`,
`ObjectBuyItem`, `RestoreItem`, `ParcelInfo`, `ParcelUpdate`, `ParcelDetails`,
`DirLandResult`; `price_per_meter` on `RegionLimits`; `price_for_listing` on
`ClassifiedInfo`, `ClassifiedUpdate`, `DirClassifiedResult`; the
`Command::SetObjectForSale.sale_price` field + `Session::set_object_for_sale`
param. **Design divergence (user-directed, this session):** the roadmap said
"wrap/unwrap at the codec boundary so wire bytes are byte-identical", which
implied an *infallible* wrap. Instead the codec is **fallible and does not
mask** — the user rejected the `LindenAmount(u64::try_from(raw).unwrap_or(0))`
pattern (it would silently rewrite a malformed negative price to `0`). New
`WireError::ValueOutOfRange { field, value }` in `sl-wire`; new
`crate::types::linden_from_wire`/`linden_to_wire` boundary helpers. A negative
wire L$ value (which a conforming peer never sends) now *rejects* the message:
the `Result`-returning struct decoders (`economy_data`, `region_limits`,
`parcel_info`, `classified_info`, `object_properties`, the three
`inventory_item*` builders) propagate the error so the datagram is dropped
(and
surfaced as a `DecodeFailed` diagnostic on the normal path), and the
`Option`-returning LLSD decoders (`parcel_info_from_llsd`,
`inventory_item_from_llsd`, `bulk_update_item_from_llsd`) reject via `None`
(a `CapsDecodeFailed` / dropped item). `try_dispatch_object` became
`Result<bool, Error>`. On the encode side the same helper rejects a value
above
the signed-32-bit wire range rather than clamping, so the public server-side
`*_to_llsd` encoders (`parcel_info_to_llsd`, `inventory_descendents_to_llsd`,
`bulk_update_inventory_to_llsd`, `ais_inventory_update_to_llsd`,
`inventory_item_to_llsd`, `bulk_update_item_to_llsd`) now return
`Result<Llsd, WireError>`. Wire bytes remain byte-identical for all valid
(non-negative, in-range) values. **Scope widened (user follow-up): also
converted the non-negative L$ fields the roadmap didn't name explicitly** —
parcel `claim_price`/`rent_price`/`pass_price` (`ParcelInfo`/`ParcelUpdate`),
group `membership_fee` (`GroupProfile`/`CreateGroupParams`),
`PlacesResult.price`, and the `GroupAccountSummary` non-negative block
(`total_credits`/`total_debits`/the four `*_tax_current`/the four
`*_tax_estimate`/`parcel_dir_fee_current`/`parcel_dir_fee_estimate`) — making
`group_membership`/`group_member` stay infallible but
`group_profile`/`group_account_summary`/
`parcel_info`-LLSD fallible and the `PlacesReply`/`GroupAccountSummaryReply`
server encoders + `parcel_info_to_llsd` encode them via `linden_to_wire`.
**Left raw (deliberately):** the genuinely *signed* group `balance` (both
`GroupProfile.money` and `GroupAccountSummary.balance`) and `amount`
(`GroupAccountDetailsEntry`, `GroupAccountTransaction` — the latter doc'd
"positive credit, negative debit") fields — those are the *next* roadmap item,
`LindenBalance`. **Also corrected a pre-existing mislabel:** group
`contribution` (`GroupMembership`/`GroupMember`) is *not* L$ at all — the wire
`Contribution` is the member's **land-tier donation in square metres** (the
viewer renders it as `[AREA]`, confirmed in Firestorm
`llpanelgrouproles.cpp`/`llfloaterlandholdings.cpp`), so it stays `i32` and
its "L$" doc comments were fixed. NO sl-types change (consume-only —
`LindenAmount`
already has the needed traits). REPL parses the raw `u64` then wraps;
`sl-survey`'s JSON record `sale_price` is now a `u64` (`info.sale_price.0`);
both runtimes + the tokio examples updated at parity. Book
`content/economy.md` updated. +3
focused unit tests (`linden_from_wire`/`linden_to_wire` round-trip
bit-identical for non-negative values incl. `0`/`i32::MAX`; negative rejected;
over-`i32` encode rejected); lifecycle + `sim_session` + conversions
round-trip
suites updated (clippy `--all-targets` clean, `cargo doc -D warnings` + mdbook
green).
**Follow-ups (same session, user-directed):** (1) **`LandArea(u32)` newtype**
— the wire carries land *area* (square metres) in the same signed-32-bit slots
L$ prices use, and group `contribution` was even doc-mislabelled "L$" when it
is land tier in m² (viewer `[AREA]`, confirmed in Firestorm). Added a public
`LandArea(pub u32)` (Display "N m²", `Add`/`Sub`, transparent serde) in
`sl-proto/src/types/land_area.rs` with `land_area_from_wire`/`_to_wire`
boundary helpers (reject negative, same as the L$ helpers). Typed every
land-area field: group `contribution` (×2), `MoneyBalance`
`square_meters_credit`/`_committed`, `ParcelInfo.area`,
`ParcelDetails`/`PlacesResult`/`DirLandResult` `actual_area`/`billable_area`.
**Kept client-local in `sl-proto` (NOT `sl-types`)** per the user, to be moved
to `sl-types` with the other value types in one later batch (avoids version
churn) — same precedent as the union keys. (2) **Sale prices →
`Option<LindenAmount>`** (`None` = not for sale, gated on the companion
`sale_type`/`FOR_SALE` flag/`for_sale` field; a for-sale item may still be
free; wire `0` is the not-for-sale sentinel) on `ObjectProperties`/
`ObjectPropertiesFamily`, `InventoryItem`, `RestoreItem`,
`ParcelInfo`/`ParcelUpdate`/`ParcelDetails`, `DirLandResult`, and
`Command::SetObjectForSale`; new `linden_price_from_wire`/`_to_wire` helpers
do the gating. **`ObjectBuyItem.sale_price` was a latent `i32` the first sweep
missed** (its doc said "advertised" not "the sale price") — fixed to plain
`LindenAmount` (the bid you must match; lost its `Copy` derive). (3) **Added a
`serde` dependency to `sl-proto`** (user-directed) so `LandArea` derives
transparent serde and `sl-survey`'s JSON record carries the typed
`area: LandArea` / `sale_price: Option<LindenAmount>` directly (was raw
`u32`/`u64`); JSON output is unchanged (transparent newtypes). The free-fn
boundary helpers stay `pub(crate)` (not inherent methods) precisely so the
value types migrate to `sl-types` cleanly without dragging
`sl_wire::WireError` along — same reason `LindenAmount`'s converter is a free
fn. +2 focused unit
tests (`LandArea` round-trip + reject-negative; sale-price for-sale gating).
