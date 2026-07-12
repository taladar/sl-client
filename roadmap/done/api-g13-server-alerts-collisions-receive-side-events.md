---
id: api-g13
title: Server alerts & collisions (receive-side events)
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G13 — Server alerts & collisions (receive-side events)

Decode currently-dropped notifications into events: `AlertMessage`,
`AgentAlertMessage`, `MeanCollisionAlert`, `HealthMessage`, `CameraConstraint`.
All receive-only; server side gets matching `send_*` encoders. OpenSim-testable.

- [x] G13 alerts, collisions, health, camera-constraint events. Five
  receive-only `Event`s decoded in the client dispatch
  (`session/methods.rs`): `AlertMessage { message, alert_info, agents }`
  (reusing the existing `AlertInfo` type for the localizable keys),
  `AgentAlertMessage { agent_id, modal, message }`,
  `MeanCollisionAlert(Vec<MeanCollision>)`, `HealthMessage { health }`, and
  `CameraConstraint { plane: [f32; 4] }`. New type module
  `sl-proto/src/types/alert.rs`: `MeanCollision` (victim/perp/time/magnitude +
  `MeanCollisionType`, the 6-variant `EMeanCollisionType` from
  `mean_collision_data.h`). Server: `SimSession` gains the five matching
  encoders `send_alert_message`/`send_agent_alert_message`/
  `send_mean_collision_alert`/`send_health_message`/`send_camera_constraint`.
  Both runtimes forward all `Event`s generically (the three login/survey
  examples gained the new variants in their ignore arms) + REPL (5 event names
  in `format.rs`). Tests: 4 lifecycle client (alert pair, mean collision,
  health + camera constraint surface) + 1 loopback round-trip (all five sim→
  client). Book: `content/world.md` gained a "Simulator notifications" section +
  "In this codebase" entry. OpenSim-testable (standard simulator notifications)
  but NOT live-tested this session (loopback + lifecycle cover both directions).
  **Scope note:** all five are receive-only — no client command and no
  `ServerEvent` (the client never sends them); `ViewerFrozenMessage` (Low 137)
  is out of scope (not listed in G13). **NEXT = G14** (SimulatorFeatures +
  AgentPreferences CAPS).
