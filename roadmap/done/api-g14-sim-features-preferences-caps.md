---
id: api-g14
title: Sim features & preferences (CAPS)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G14 — Sim features & preferences (CAPS)

`SimulatorFeatures` capability (mesh/feature flags, voice server type, etc. —
parse into a `SimulatorFeatures` type surfaced at handshake); `AgentPreferences`
capability (get/set hover height, default perms, etc.). SL-only emphasis;
OpenSim advertises a subset of `SimulatorFeatures`.

- [x] G14 SimulatorFeatures + AgentPreferences. Two HTTP CAPS, both hand-written
      LLSD codecs in `sl-wire`: `sim_features.rs` (`SimulatorFeatures` +
      `PhysicsShapeTypes`/`AnimatedObjects`/`OpenSimExtras` subtrees;
      `parse_simulator_features` / `build_simulator_features_response`) and
      `agent_preferences.rs` (`AgentPreferences` + `ObjectPermMasks`,
      all-`Option` fields so a partial POST is a partial update and an empty
      POST is a "get"; `build_agent_preferences_request` /
      `parse_agent_preferences` / `build_agent_preferences_response`). New caps
      `CAP_SIMULATOR_FEATURES` + `CAP_AGENT_PREFERENCES` (both added to
      `REQUESTED_CAPABILITIES`). Commands `RequestSimulatorFeatures` (explicit
      re-fetch GET), `RequestAgentPreferences` (POST empty body → "get"),
      `SetAgentPreferences(Box<AgentPreferences>)` (POST the changed fields);
      events `SimulatorFeatures(Box<…>)` + `AgentPreferences(Box<…>)` decoded in
      `handle_caps_event`. **SimulatorFeatures is fetched automatically** at
      handshake: both runtimes GET it once the capability map is known (at login
      and on each region change) — tokio via a new
      `caps::spawn_simulator_features`, bevy in the `map_rx` drain — with no
      command needed, matching the viewer. Both runtimes + REPL
      (`request_simulator_features`, `request_agent_preferences`,
      `set_agent_preferences` with keyword fields) + format.rs event/command
      names. Tests: 4 wire round-trip (2 per module) + 2 proto
      `handle_caps_event` decode + 4 REPL registry. Book: extended
      `content/region.md` with "Simulator features" and "Agent preferences"
      sections + "In this codebase". **Scope note:** these are CAPS-only (HTTP,
      out-of-band), so — like G3/G7 — the server side is the `build_*_response`
      wire functions, with no UDP `SimSession` encoder or `ServerEvent`. SL-only
      emphasis (the SL-only PBR/GLTF flags); OpenSim advertises a subset of
      `SimulatorFeatures` (plus its `OpenSimExtras` map) and serves
      `AgentPreferences`, so both are partially OpenSim-testable, but NOT
      live-tested this session (wire + lifecycle round-trips cover both
      directions). **NEXT = G15** (object/attachment/land resource costs and
      physics data).
