---
id: permission-a4
title: Decide the auto-act policy (API-concept grants)
topic: permission
status: done
origin: PERMISSION_ROADMAP.md
---

Context: [context/permission.md](../context/permission.md).

**A4. Decide the auto-act policy (API-concept grants).** Using A1, decide
which granted permissions the session acts on autonomously versus surfaces for
the library user: record-only flags need no action; controls/camera are
surfaced (the user cooperates) and the session only tracks the taken-controls
set from `ScriptControlChange`; for `TELEPORT`, decide whether a granted
script-teleport (`Event::ScriptTeleport`) may auto-call `teleport_to` or stays
user-driven. Keep the library a conduit where it must be, a convenience where
the API already covers the action. **Done — see § Auto-act policy reference
(from A4) + the B1 amendment in § Phase B.** Decided: the session takes **no
autonomous action** on any granted permission — every flag is either a
*record-only* mirror (the sim enforces) or a *cooperation* surface (the
runtime routes inputs / camera). **Correction to A1's premise:** `TELEPORT` is
**not** client-actionable. A granted `llTeleportAgent` is executed
*server-side* (`DoLLTeleport → World.RequestTeleportLocation`) and reaches the
client as an ordinary teleport already handled by `TeleportPhase`;
`Event::ScriptTeleport` is `llMapDestination` — a map beacon that needs **no**
permission and must **not** auto-call `teleport_to`. So `TELEPORT` is
reclassified *record-only*, there are **zero** auto-act flags, and B1's
`PermissionRole` drops its `ApiAction` variant (now two roles).

## Auto-act policy reference (from A4)

The decision: **the session takes no autonomous action on any granted
permission.** Every one of the 12 flags is either *record-only* (the sim
enforces end-to-end; the client mirrors the grant and any effect arrives on the
ordinary message path) or *cooperation* (inert until the runtime routes control
inputs or applies camera params). No grant maps onto a client-initiated
`Session` method — so there is nothing for A4 to "auto-act". This keeps the
library a pure conduit/mirror and leaves all policy (whether to cooperate, when
to revoke) to the driver.

**Why `TELEPORT` is not an action (the A1 correction).** A1 drafted `TELEPORT`
as "API action → `Session::teleport_to` via `Event::ScriptTeleport`". The
protocol disproves both halves:

- A granted `llTeleportAgent` / `llTeleportAgentGlobalCoords` runs
  **server-side**: OpenSim's `DoLLTeleport` calls
  `World.RequestTeleportLocation` / `RequestTeleportLandmark`, i.e. the sim
  teleports the agent itself. The client receives a normal teleport
  (`TeleportStart` → `TeleportLocal` / region handoff → `TeleportFinish`)
  already driven by `TeleportPhase`. There is no client-initiated step, so
  a granted `TELEPORT` needs **no** auto-act — it is *record-only*.
- `Event::ScriptTeleport` (`ScriptTeleportRequest`, from `llMapDestination`) is
  a **map beacon that requires no permission at all** — Firestorm's
  `process_script_teleport_request` only tracks the location on the world-map
  floater (gated on `ScriptsCanShowUI`), it does **not** teleport. It is
  unrelated to the `TELEPORT` grant. The session must therefore **not**
  auto-call `teleport_to` on it; it stays a passthrough event the driver may act
  on (open a map, offer a teleport) entirely at its discretion.

**Cooperation flags — surfaced, never auto-acted.** `TAKE_CONTROLS` is surfaced
via `Event::ScriptControlChange`; the runtime routes the avatar's control inputs
and `sl-proto` only mirrors the live *taken-controls* set (the A6 tracker, fed
by `ScriptControlChange` Take/Release). `TRACK_CAMERA` / `CONTROL_CAMERA` are
surfaced via the follow-cam events (`FollowCamProperty` /
`FollowCamPropertyValue`); the runtime applies the camera params. The session
records the grant but initiates nothing.

**Consequence for the registry.** A4 changes only *roles/policy*, not storage:
the registry still stores all granted bits wholesale (B2 unchanged). Because
there are now **zero** auto-act flags, B1's `PermissionRole` enum collapses from
three variants to two (`RecordOnly` / `Cooperation`) — see the B1 amendment.

A4 produces **no new implementation task**: "no autonomous action" is the
absence of code. Its only code-facing output is the B1 amendment below; the
`Event::ScriptTeleport` passthrough already exists and is intentionally left
untouched.
