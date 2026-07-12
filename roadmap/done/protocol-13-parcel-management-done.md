---
id: protocol-13
title: Parcel management (done)
topic: protocol
status: done
origin: ROADMAP.md — Tier B
---

Context: [context/protocol.md](../context/protocol.md).

**13. Parcel management (done) ✅ — `ParcelPropertiesUpdate`,
`ParcelAccessListRequest`/`Reply`/`Update`, `ParcelDwellRequest`/`Reply`,
`ParcelBuy`, `ParcelReturnObjects`, `ParcelSelectObjects`, plus
`ParcelDeedToGroup`/`Reclaim`/`Release` · 5 pts.** Turns the existing parcel
read path into a land-management tool. `Session` gains
`update_parcel(&ParcelUpdate)` (a builder-style struct — flags, name/desc,
category, sale price, group, media, landing point),
`request_parcel_access_list`/`update_parcel_access_list` (allow/ban lists via
`ParcelAccessScope`, surfaced as `Event::ParcelAccessList`),
`request_parcel_dwell` (→ `Event::ParcelDwell`), `buy_parcel`,
`return_parcel_objects`/`select_parcel_objects` (`ParcelReturnType` bitfield),
`deed_parcel_to_group`, `reclaim_parcel`, `release_parcel`. Wired through both
runtimes. Added a `ParcelFlags::union` helper for combining flags. **Fixed a
pre-existing CAPS read bug:** OpenSim encodes the `uint` `ParcelFlags` as a
4-byte binary LLSD element, which the old `as_i32` parse dropped to `0` — now
read via a tolerant `llsd_u32` (binary/integer/string). **Read-side
stream/media URLs (follow-up):** `ParcelInfo` now also surfaces the parcel's
streaming-audio URL (`music_url`), media URL (`media_url`), `media_id` and
`media_auto_scale` — the `ParcelProperties` message and CAPS event already
carried them, but the old `parcel_info`/`parcel_info_from_llsd` builders dropped
them, so a client could *set* a parcel's stream URL (via `ParcelUpdate`) but not
*read* the current one. Both decode paths covered by tests (the UDP wire form
incl. NUL-trimming, and the CAPS LLSD form incl. OpenSim's boolean
`MediaAutoScale`); the CAPS LLSD keys (`MusicURL`/`MediaURL`/`MediaID`/
`MediaAutoScale`) were cross-checked against OpenSim's
`LLClientView.cs` encoder. (The *per-face* media-on-a-prim system and parcel
media *control* — `ObjectMedia`/`ObjectMediaNavigate`,
`ParcelMediaCommandMessage` — remain roadmap #24.) *Live-verified against
local OpenSim logged in as the estate owner: dwell read, access-list read +
write + re-read round-trip, and a `ParcelPropertiesUpdate` that changed the
parcel name and flags (confirmed via the console and across logins; OpenSim
serves an explicit in-session ParcelProperties re-request from a cached
snapshot, so flag edits show on the next fetch). Most write ops need parcel
ownership / estate powers — see the estate-owner login.*
