---
id: idiomatic-p6-04
title: map::RegionName(String) for genuine region-name fields
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`map::RegionName(String)` — only for genuine *region* name fields (region
info / map block replies / teleport). Consumed the existing
`sl_types::map::RegionName` nutype (validation `len 2..=35` after trim, the SL
wiki limit — **no `sl-types` change**, consume-only). Audited every `name:
String` / `sim_name` / `region_name` site and converted the **11 genuine
region-identity fields** to **`Option<RegionName>`** (`None` = the empty
"unknown region" sentinel — same precedent as the Phase-5 nil-sentinel
`Option`s): `RegionIdentity.sim_name`, `RegionLimits.sim_name`,
`MapRegionInfo.name`, `ParcelDetails.sim_name`, `PickInfo.sim_name`,
`ClassifiedInfo.sim_name`, `PlacesResult.sim_name`, `EventInfo.sim_name`,
`ScriptTeleportRequest.region_name`, plus the two sl-wire carriers
`ParcelVoiceInfo.region_name` and `AbuseReport.abuse_region_name`.
**Codec boundary is FALLIBLE + non-masking** (user-chosen via AskUserQuestion,
mirroring the `LindenAmount` precedent): new public sl-wire helpers
`region_name_from_wire(field, raw) -> Result<Option<RegionName>, WireError>`
(empty/whitespace → `Ok(None)`; non-empty invalid → new
`WireError::InvalidRegionName`) and `region_name_to_wire(Option<&RegionName>)
-> String` (in `sl-wire/src/region_name.rs`), so wire bytes are byte-identical
for valid names. **A non-empty invalid name is never silently dropped** (user
requirement): the UDP struct decoders propagate the error up through
`dispatch`/`handle_datagram` as a **hard error**; `map_region_info` was made
fallible (`Result<Option<_>, WireError>`) so a bad map-block entry is a hard
error too (empty/sentinel entries still skip via `Ok(None)`); the caps
`ParcelVoiceInfo::from_llsd` `None` already routes to a
`Diagnostic::CapsDecodeFailed`; the server-side caps `parse_send_user_report`
became `Result<AbuseReport, WireError>`. `region_identity`/`pick_info` were
made fallible; the empty-string `Default`s on `ParcelDetails`/`AbuseReport`/
`ParcelVoiceInfo` became `None`. **Left raw (deliberately):** the polymorphic
`MapItem.name` (region/parcel/event/avatar-hash) and the two *outbound search
filters* `DirPlacesQuery.sim_name`/`PlacesQuery.sim_name` (possibly-partial
query strings, not region identities), plus all person/object/inventory/
estate/event/parcel names. Re-exported `RegionName` +
`region_name_from_wire`/`region_name_to_wire` through
`sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL parses the raw
arg then wraps (mapping a bad name to `ReplError::InvalidArg`); `sl-survey`
renders the `Option<RegionName>` to its raw-`String` JSON record; examples use
`{:?}`. +3 boundary unit tests (`region_name.rs`: empty→`None` round-trip,
valid round-trip, non-empty invalid rejected); lifecycle + `sim_session` +
voice/abuse round-trip suites updated. NO `sl-types` touched.
