---
id: viewer-combat-health-indicator
title: Combat health meter (HealthMessage)
topic: viewer
status: ready
origin: script-interface survey (2026-07-23)
refs: [viewer-ui-status-bar, viewer-ui-status-bar-parcel-icons, api-g13]
---

Context: [context/viewer.md](../context/viewer.md).

On damage-enabled ("not safe") land the simulator tracks avatar health;
scripts deal damage (`llSetDamage`, collisions) and the sim sends
`HealthMessage` with the current health percentage. `sl-proto` decodes it
(`Event::HealthMessage`) but nothing consumes it, so the viewer shows no
health at all in combat areas.

Scope:

- Consume `Event::HealthMessage` into an own-avatar health resource.
- Status-bar health indicator (heart icon + percentage), shown only when
  the current parcel/region has damage enabled (the parcel damage flag
  the status-bar parcel icons already read), hidden elsewhere.
- Reset handling on teleport/region change and on "death" (health 0 →
  the sim teleports the agent home; surface the transition rather than a
  stale meter).

Reference (Firestorm, read-only): `process_health_message`
(`llstartup.cpp` registry), the status-bar health display
(`llstatusbar.cpp`).

Builds on: the status bar (done) and parcel-flag plumbing (done);
receive-side alert/collision events are [[api-g13]].
