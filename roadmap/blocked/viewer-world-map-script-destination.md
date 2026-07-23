---
id: viewer-world-map-script-destination
title: Script-requested map destination (llMapDestination)
topic: viewer
status: blocked
origin: script-interface survey (2026-07-23)
blocked_by: [viewer-world-map-tracking-teleport]
refs: [viewer-world-map-floater, viewer-beacons-beam-render]
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

Builds on: world-map tracking + teleport-from-track (the blocked map
task provides the tracking state this reuses).
