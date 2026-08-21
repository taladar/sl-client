---
id: viewer-fake-grid-teleport
title: Fake-grid inter-region teleport (teleport_agent + auto-responder)
topic: viewer
status: done
origin: follow-up of viewer-fake-grid (2026-08-21) — the open teleport helper
points: 5
refs: [viewer-fake-grid, viewer-fake-grid-login-smoke, protocol-sim-udp-flows,
  protocol-10,
  test-handover-distant-and-vehicle-aditi, server-agent-transfer,
  viewer-own-avatar-broken-after-teleport, viewer-crossing-movement-locks-up]
---

Context: [context/viewer.md](../context/viewer.md).

The fake grid ([[viewer-fake-grid]]) was N-region from the start but only
single-region sessions were exercised; the cross-region teleport — the
hardest client flow, testable only on aditi until now
([[test-handover-distant-and-vehicle-aditi]]) — had no grid side. This
task is the I/O realisation of `sl-proto/tests/sim_session.rs`'s
`inter_region_teleport_two_sims` in `sl-fake-grid`: a second
socket/`SimSession`/`SimCaps` in the destination region, the event-queue
trio, and the source's retirement.

## Done (2026-08-22)

- **`sl-fake-grid/src/teleport.rs`**: `teleport_session` sequences one
  teleport like OpenSim's `EntityTransferModule.TransferAgent_V2`
  (`TeleportStart` + progress keys → destination session prepared,
  placed and registered *before* it is announced → `EnableSimulator` +
  `EstablishAgentCommunication` + `TeleportFinish` → wait for the
  destination's `AgentArrived` → retire the source). A per-session
  **responder task** answers the client's own requests: by handle, by
  landmark (asset store → `sl_wire::parse_landmark` → region id), home
  (the account's start region), and lure (OpenSim's fake-parcel-id
  convention, `sl_wire::FakeParcelId`; an opaque id = the lurer's agent
  id). Refusals use the `teleport_strings.xml` keys (`invalid_tport`,
  `nolandmark_tport`, `no_host`, `timeout_tport`) so a viewer's screen
  never hangs; a same-region request is a `TeleportLocal`. Public API:
  `FakeGrid::teleport_agent` (grid-initiated, returns the destination
  `FakeAgent`), `teleports()` / `TeleportNotice`, `agent_by_seq`,
  `region_handle` / `region_id` / `region_names`, `FakeAgent::{session_seq,
  is_closed}`. The binary's `--region` repeats (`Name@X,Y`).
- **Runtime refactor**: `prepare_region_session` (shared by login and
  teleport; `SessionIds` carries the login-minted session/secure/circuit
  ids every circuit reuses), `SimState::{seq, region, ids}`, a per-session
  `closed` watch so the pumps exit on retirement/abandonment instead of
  the inactivity timeout.
- **sl-proto**: `TeleportFinishInfo` — the full reference `TeleportFinish`
  record (`AgentID`, `LocationID`, `RegionHandle`, `RegionSizeX/Y`; the
  old builder omitted them and Firestorm builds the destination region
  from the handle); `EnableSimulator` gains the region size,
  `EstablishAgentCommunication` the `agent-id`. The client decoder reads
  the wire `RegionHandle` (a lure/landmark teleport never knew its
  target; before it reported `RegionHandle(0)`). `SimSession::{set_arrival_
  position, retire_circuit, abandon}`, `ServerEvent::CircuitRetired`,
  `ArrivalPlacement`, `teleport_strings`.
- **Client bug fixed en route**: a **server-initiated** teleport
  (`llTeleportAgent`, god/estate teleport-home, a grid push) was ignored
  — `TeleportStart` outside a requested teleport was dropped on the
  theory that crossings send it (OpenSim's `EntityTransferModule` sends
  it on its two teleport paths only), so the following `TeleportFinish`
  stranded the session. The client now enters the teleport on a remote
  `TeleportStart` and on a finish-without-start, as Firestorm does
  (`process_teleport_start` / "Teleport 'finish' message without
  'start'"). Loopback test `remote_initiated_teleport_two_sims`.
- **sl-wire**: `landmark` codec (both on-wire versions; the viewer's
  `parse_landmark` now delegates) and `FakeParcelId`.
- **Tests**: 9 real-client e2e tests in `client_end_to_end.rs` (request,
  unknown region, same region, grid-initiated, landmark, unknown
  landmark, home, lure by fake parcel id + opaque lure, helper error);
  sl-proto loopback + conversion round-trips. Binary smoke with
  `sl-repl-tokio`: `teleport_started → resolving / sending_dest /
  arriving → neighbor_seed → teleport_finished (east handle) →
  region_info_handshake "Fake Region East" → region_changed`, source
  session retired grid-side.

## Findings

- The client keeps the scene on a teleport whose destination was
  announced first (`EnableSimulator` makes it a child circuit before the
  finish), so `RegionChanged { world_reset: false }` is the expected
  outcome even for a distant hop — the reset only happens for a
  destination that was neither a child nor adjacent.
- After a teleport the client emits `RegionChanged`, not a second
  `RegionHandshakeComplete`; the destination's `RegionInfoHandshake`
  arrives on the child circuit *before* `TeleportFinished`.
- UDP progress lines can outrun the CAPS seed in the client's event
  order; assert keys, not the interleaving.

## Follow-ups

- `CrossedRegion` (a walked border crossing) — the fake grid has no
  movement authority; a scripted crossing helper would mirror
  `teleport_session` with `enqueue_crossed_region`.
- Neighbour child agents on arrival (`EnableSimulator` for adjacent
  regions) — interest management, out of scope here.
- Lure *offers* are not routed between sessions (no inter-client IM
  delivery in the fake grid); tests mint the lure id directly.
- The two open teleport bugs ([[viewer-own-avatar-broken-after-teleport]],
  [[viewer-crossing-movement-locks-up]]) now have a loopback harness.
