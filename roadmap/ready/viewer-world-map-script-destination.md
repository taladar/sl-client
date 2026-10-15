---
id: viewer-world-map-script-destination
title: Script-requested map destination (llMapDestination)
topic: viewer
status: ready
origin: script-interface survey (2026-07-23)
blocked_by: [viewer-world-map-floater, viewer-beacons-beam-render]
refs: [viewer-world-map-tracking-teleport]
---

Context: [context/viewer.md](../context/viewer.md).

`llMapDestination` sends `ScriptTeleportRequest`: a script asks the
viewer to open the world map at a named region + local coordinates —
the mechanism behind teleport boards, vendor "visit the main store"
buttons, and hunt hint-givers. `sl-proto` decodes it
(`Event::ScriptTeleport`) but nothing consumes it.

Scope:

- On `Event::ScriptTeleport`, open the world-map floater centred on the
  named region/position and set a tracking marker there (the same
  track-and-teleport state the map's own click-tracking uses).
- Show the in-world tracking beacon/arrow toward the destination.
- Follow the reference's rate limiting (repeated requests replace the
  current track rather than stacking).

Reference (Firestorm, read-only): `process_script_teleport_request`
(`llviewermessage.cpp`), `LLWorldMapMessage` tracking glue.

Builds on: world-map tracking + teleport-from-track.

Audit 2026-10-15: unblocked. The tracking state this reuses exists without
[[viewer-world-map-tracking-teleport]] finishing — the shared `MapTracking`
target, which the map's click sets and the in-world beacon
(`sl-viewer-world-scene/src/beacons.rs`) draws. A region-name destination can go
through the same path a `secondlife:///app/worldmap/...` SLURL takes:
`sl-viewer-places/src/slurl_dispatch.rs` resolves the region name (a named
map-block request) and opens the map there (`LocationAction::ShowOnMap`,
`OpenWorldMap`). `Event::ScriptTeleport` (`ScriptTeleportRequest`) still has no
consumer.
