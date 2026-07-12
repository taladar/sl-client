---
id: missing-out-batch-7
title: teleport & agent prefs
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

**Out batch 7 — teleport & agent prefs.** `TeleportLandmarkRequest` (teleport
to a landmark), `TeleportCancel` (cancel an in-progress teleport),
`SetStartLocationRequest` (set home), `AgentDataUpdateRequest`,
`AgentQuitCopy` (crash-quit leaving objects), `VelocityInterpolateOn` /
`VelocityInterpolateOff`.

Implemented as `Session::teleport_via_landmark(landmark: Option<AssetKey>)`
(the `LandmarkID` is the landmark inventory item's *asset* id, `None` =
home; mirrors `teleport_to`'s state machine — arms the teleport timeout and
enters [`TeleportPhase::Requested`] with no destination hint, since a
landmark teleport resolves sim-side and the authoritative handle arrives with
the `TeleportFinish`), `Session::cancel_teleport` (sends `TeleportCancel` and,
if teleporting, returns to the active state and disarms the timeout),
`Session::set_start_location(slot: StartLocationSlot, position:
RegionCoordinates, look_at: Vector)`, `Session::request_agent_data_update`,
`Session::quit_copy`, and `Session::set_velocity_interpolation(enabled:
bool)` (one method dispatching to the On/Off messages). The only new domain
type is a typed [`StartLocationSlot`] enum (`Last`/`Home`/`Direct`/`Parcel`/
`Telehub`/`Url`, the reference viewer's `EStartLocation` ordinal) replacing
the raw `LocationID` `u32` — kept distinct from the existing login
[`StartLocation`] (the SLURL-style `start=` parameter, a different shape that
bundles region+position and has no wire ordinal); the `SimName` is sent empty
(the simulator fills the region name, as the reference viewer does) and
`AgentQuitCopy`'s `ViewerCircuitCode` reuses the circuit's own code. Wired as
`Command::{TeleportViaLandmark, CancelTeleport, SetStartLocation,
RequestAgentDataUpdate, QuitCopy, SetVelocityInterpolation}` through the tokio
and bevy runtimes, the `command_name` formatter, and the matching REPL tokens
(with a `parse_start_location_slot` helper). Covered by three pack-the-wire
lifecycle tests, one `StartLocationSlot` round-trip unit test, and three REPL
parse tests. Teleport-to-landmark/home, cancel, set-home, agent-data poll,
and velocity interpolation are OpenSim-testable; `AgentQuitCopy` is an
inter-sim quit best exercised against SL.
