---
id: api-g9
title: Task-script control
topic: api
status: done
origin: SL_API_ROAD_MAP.md
---

Context: [context/api.md](../context/api.md).

## G9 — Task-script control

`GetScriptRunning`/`ScriptRunningReply`, `SetScriptRunning`, `ScriptReset`,
`ScriptSensorRequest`/`ScriptSensorReply`. (The existing `RequestTaskInventory`
CAPS already lists a prim's scripts; this adds run/stop/reset/sensor.)
OpenSim-testable (XEngine + scripted object).

- [x] G9 task-script run/stop/reset. Commands `RequestScriptRunning`
  (`GetScriptRunning` → `Event::ScriptRunning`, the `ScriptRunningReply`
  run-state; note this request carries no `AgentData` block), `SetScriptRunning`
  (start/stop), `ResetScript` (`ScriptReset`). Circuit encoders
  `send_get_script_running`/`send_set_script_running`/`send_script_reset` +
  `Session` methods `request_script_running`/`set_script_running`/
  `reset_script`; `ScriptRunningReply` decoded in the dispatch path. Server:
  each inbound message decodes into a matching `ServerEvent`
  (`RequestScriptRunning`/
  `SetScriptRunning`/`ResetScript`) plus
  `SimSession::send_script_running_reply`. Both runtimes + REPL (3 commands) +
  format.rs. Tests: 2 lifecycle client (commands encode, reply decode) + 1
  loopback round-trip + 3 REPL registry. New book chapter `content/scripts.md`
  (+ SUMMARY entry) covering task-script control and the existing
  dialog/permission events. **Scope note:** `ScriptSensorRequest`/
  `ScriptSensorReply` (Low 247/248, the `llSensor` family) are **not** wrapped —
  they are `Trusted` simulator↔dataserver messages the viewer never sends or
  receives (handled by neither the Firestorm viewer nor OpenSim's
  `LLClientView`), following the G1/G4/G5/G6/G7/G8 trusted-backend precedent;
  they remain reachable as a raw `AnyMessage` if ever needed. OpenSim-testable
  (XEngine + scripted object) but NOT live-tested this session (loopback +
  lifecycle tests cover both directions). **NEXT = G10** (group finance,
  proposals, voting).
