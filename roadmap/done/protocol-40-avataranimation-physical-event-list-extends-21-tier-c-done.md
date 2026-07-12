---
id: protocol-40
title: AvatarAnimation physical-event list (extends #21, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**40. `AvatarAnimation` physical-event list (extends #21, Tier C). ✅ Done.**
The `AvatarAnimation` handler (`session.rs`) read `animation_list` and
`animation_source_list` but never the `PhysicalAvatarEventList` block, which the
codec decodes into the struct and the handler then dropped. Surfaced it as a new
**`physical_events: Vec<Vec<u8>>`** field on `Event::AvatarAnimation` — one
opaque `TypeData` byte blob per block, **verbatim, not decoded**: neither the
reference viewer's `process_avatar_animation` (which reads only the two
animation lists and ignores this block) nor OpenSim (which never populates it)
assigns the payload any documented structure, so a faithful surface is the raw
bytes (almost always empty). Re-exported through both runtimes via the shared
`Event` type; the `tokio_login_hold_logout` example now logs the block count.
Covered by the extended `avatar_animation_surfaces_playing_animations`
`sl-proto` lifecycle test (a populated single block round-trips to
`physical_events == [[0xDE, 0xAD, 0xBE, 0xEF]]`). *Live-verified against the
local OpenSim via `tokio_login_hold_logout`: a `PlayAnimation` round-trip echoed
`Event::AvatarAnimation` with 2 animations and `0 physical event block(s)` —
OpenSim sends an empty `PhysicalAvatarEventList`, so the field is empty as
designed, confirming the path decodes end-to-end with no protocol error. Test:
local OpenSim.*
