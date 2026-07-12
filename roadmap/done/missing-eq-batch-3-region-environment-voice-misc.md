---
id: missing-eq-batch-3
title: region/environment/voice misc
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**EQ batch 3 — region/environment/voice misc.** `WindLightRefresh` (re-fetch
environment), `SimConsoleResponse` (reply to a region debug-console command),
`RequiredVoiceVersion` (voice protocol version), `OpenRegionInfo` (OpenSim
extended region settings).

Implemented as two inline `Event` variants and two struct-carrying ones:
`Event::WindLightRefresh { interpolate: bool }` (the body's single
`Interpolate` int flag — the sim asks the client to re-fetch the region
environment, interpolating the transition when set) and
`Event::SimConsoleResponse { output: String }` (the body is a *bare* LLSD
string — the console command's raw output — not a map);
`Event::RequiredVoiceVersion(RequiredVoiceVersion)` where
`RequiredVoiceVersion { major_version: i32, region_name: String,
voice_server_type: Option<String> }` lives in the new `types/voice.rs` (the
voice backend is `None` on older grids, which the reference viewer treats as
the `"vivox"` default); and
`Event::OpenRegionInfo(Box<OpenRegionInfo>)` where `OpenRegionInfo` (new
`types/open_region.rs`) is a 27-field all-`Option` bag of OpenSim per-region
limits/overrides — only the keys the sim sends are present, matching the
reference viewer's independent `has()` checks. The `Max`/`Min` position bounds
group their `*PosX`/`*PosY`/`*PosZ` keys into a `RegionCoordinates` (present
only when all three components are); other fields stay primitive (no domain
newtype fits these OpenSim-specific limits).
`WindLightRefresh`/`SimConsoleResponse` are OpenSim-emitted; `OpenRegionInfo`
is OpenSim-only; `RequiredVoiceVersion` is SL/grid-specific. Decoded by
`windlight_refresh_from_llsd` / `sim_console_response_from_llsd` /
`required_voice_version_from_llsd` / `open_region_info_from_llsd` in
`session/conversions.rs` and dispatched by name in
`Session::handle_caps_event`.
