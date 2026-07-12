---
id: api-g12
title: Agent state & viewport
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G12 — Agent state & viewport

`SetAlwaysRun`, `AgentPause`/`AgentResume`, `AgentFOV`, `AgentHeightWidth`, and
receive `SetFollowCamProperties`/`ClearFollowCamProperties` plus
`ScriptControlChange`/`ForceScriptControlRelease` (scripts taking agent
controls — pairs with the existing `AnswerScriptPermissions`). OpenSim-testable.

- [x] G12 agent run/pause/FOV/window + scripted camera & controls. Five
  fire-and-forget client commands `SetAlwaysRun { always_run }`, `PauseAgent`,
  `ResumeAgent`, `SetAgentFov { vertical_angle }`, `SetAgentSize { height, width
  }` + `ReleaseScriptControls` (`ForceScriptControlRelease`); circuit encoders
  (`send_set_always_run`/`send_agent_pause`/`send_agent_resume`/
  `send_agent_fov`/`send_agent_height_width`/
  `send_force_script_control_release`) with the
  pause/resume serial as a new monotonic `Circuit::pause_serial_num` (shared by
  both, `GenCounter` fixed at 0 for FOV/height-width as the viewer always
  sends), plus `Session` methods (`set_always_run`/`pause_agent`/`resume_agent`/
  `set_agent_fov`/`set_agent_size`/`release_script_controls`). Receive side: new
  events `Event::ScriptControlChange(Vec<ScriptControl>)`,
  `SetFollowCamProperties { object_id, properties }`,
  `ClearFollowCamProperties { object_id }` decoded in the client dispatch; new
  types `ScriptControl`, `FollowCamProperty` (23-variant `EFollowCamAttributes`
  enum), and `FollowCamPropertyValue` in `sl-proto/src/types/script.rs`. Server:
  each client
  message surfaces as `ServerEvent::SetAlwaysRun`/`AgentPause`/`AgentResume`/
  `AgentFov`/`AgentHeightWidth`/`ForceScriptControlRelease`, plus encoders
  `SimSession::send_script_control_change`/`send_set_follow_cam_properties`/
  `send_clear_follow_cam_properties` for the camera/control direction. Wired
  through both runtimes + REPL (6 commands + 6 command names + 3 event names in
  `format.rs`). Tests: 6 lifecycle client (each request packs / each receive
  event surfaces), 2 loopback round-trip (agent-state client→sim,
  camera/controls sim→client), 6 REPL registry. Book: `content/appearance.md`
  gained an "Agent
  state and viewport" section + "In this codebase" entries. OpenSim-testable but
  NOT live-tested this session (loopback + lifecycle cover both directions).
  **NEXT = G13** (alerts, collisions, health, camera-constraint events).
