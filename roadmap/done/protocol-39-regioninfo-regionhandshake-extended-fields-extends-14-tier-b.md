---
id: protocol-39
title: RegionInfo / RegionHandshake extended fields (extends #14, Tier B)
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**39. `RegionInfo` / `RegionHandshake` extended fields (extends #14, Tier B). ✅
Done.** `region_identity` and `region_limits` (`session.rs`) surfaced only the
agent/object limits, maturity, product, and the 32-bit flags. Both builders now
take the whole `RegionHandshake` / `RegionInfo` message (so they can read the
optional trailing blocks) and populate the full surface. **`RegionIdentity`**
(from `RegionHandshake`) gains `sim_owner` (the region/estate owner),
`is_estate_manager` (whether *this* agent manages the estate — gates estate UI),
`water_height`, `billable_factor`, and the **64-bit `region_flags_extended`** +
`region_protocols` from the optional `RegionInfo4` block (falling back to the
zero-extended 32-bit flags / `0` when the grid sends no `RegionInfo4`).
**`RegionLimits`** (from `RegionInfo`) gains `estate_id`/`parent_estate_id`,
`water_height`, `billable_factor`, `object_bonus_factor`, `terrain_raise_limit`/
`terrain_lower_limit`, `price_per_meter`, `redirect_grid_x`/`redirect_grid_y`,
`use_estate_sun`/`sun_hour`, the 64-bit `region_flags_extended` (from the
optional `RegionInfo3` block, same fallback), and two new optional sub-structs —
`RegionChatSettings` (the `RegionInfo5` chat whisper/normal/shout ranges +
offsets + flags) and `RegionCombatSettings` (the `CombatSettings` block) —
present only when the grid sends those blocks (`None` on OpenSim and older
grids). Both value types dropped `Eq` (now `f32` fields). The three new structs
are re-exported through both runtimes; `survey_probe` already debug-prints the
whole `RegionIdentity`/`RegionLimits`. Covered by two new `sl-proto` lifecycle
tests (`region_handshake_surfaces_extended_fields` with a populated
`RegionInfo4`, and `region_info_surfaces_extended_fields` with populated
`RegionInfo3`/`RegionInfo5`/`CombatSettings`), plus the existing two tests
extended to assert the no-optional-block fallbacks. *Live-verified against the
local OpenSim via `survey_probe`: the handshake decoded `sim_owner`,
`water_height=20.0`, `is_estate_manager=false`, and a real `RegionInfo4`
(`region_protocols=0x8000000000000000`), and the `RegionInfo` reply decoded
`estate_id=101`, `parent_estate_id=1`, `terrain_raise_limit=100`/`lower=-100`,
`object_bonus_factor=1.0`, `price_per_meter=1` — OpenSim sends no
`RegionInfo3`/`5`/`CombatSettings`, so those fall back / are `None` as designed.
Test: local OpenSim.*
