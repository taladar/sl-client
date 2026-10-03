---
id: idiomatic-protocol-codes-past-decode
title: Protocol integers kept past decode where a typed enum or flags type exists
topic: idiomatic
status: ideas
origin: idiomatic-audit-primitive-typed-patterns (2026-10-03)
refs: [idiomatic-audit-primitive-typed-patterns,
  protocol-region-flag-deny-ageunverified-value]
---

Context: [context/idiomatic.md](../context/idiomatic.md).

## What

Wire codes that survive decode as integers although a type exists or should:
inventory `item_type` / `inv_type: i8`, `sale_type: u8`, `folder_type: i8 /
i32` (and `to_item` sets `sale_price: Some` for NotForSale);
`ObjectProperties*.sale_type: u8`; `ParcelUpdate.landing_type: u8` whose
`Default` 0 means Blocked; `MoneyTransaction.transaction_type: i32`; region /
estate flags `u32` / `u64`; `Event::TeleportProgress.teleport_flags: u32`
while `TeleportFinished` is typed; the server side (`SimSession` /
`ServerEvent`) less typed than the client for the same fields
(`return_type`, teleport flags, `AgentUpdateInfo.state` / `flags`,
`duplicate_flags`, `request_flags`). Flag newtypes whose constants are raw
integers (`SoundFlags`, `MuteFlags` — `contains(mask: u8)` takes anything;
`DirFindFlags` does it right). Land-stat filter bits defined in sl-fake-grid
and `top_objects.rs` separately; `ParcelMediaCommand` exposing raw flags plus
one command (the reference honours `time` whenever the TIME bit is set);
inventory item-flags subtype masked by hand (`flags & 0xff`, three places)
and `PendingItemCreation.flags: u32` meaning a wearable slot or a settings
subtype by context; experience maturity `i32` (13/21/42 re-declared in the
viewer and the fake grid — which once shipped 34 and showed Adult);
`TeleportFinishInfo.sim_access: u8`; day-cycle track `i32` in
`land_environment.rs` beside the existing `DayTrack`; link asset codes 24 / 25
in three copies; `ParcelInfoReply` flags (filed as a bug); five independent
`SaleType` encodings across crates.

Small inconsistencies found on the way: the sale price gated on
`sale_type != 0` while `SaleType::from_code` maps unknown codes to NotForSale;
`conversions.rs` falls back to `unwrap_or(0)` (Texture) where elsewhere -1.

## How

Decode into the existing enums / flags at the boundary, one shared `SaleType`,
`LandStatFilter` / `PropertiesFamilyRequest` / `ParcelMediaCommands` flags,
`InventoryItemFlags` with `subtype()`, `Maturity` for every sim-access code.
Split per area when worked.
