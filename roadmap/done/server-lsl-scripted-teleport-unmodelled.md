---
id: server-lsl-scripted-teleport-unmodelled
title: The fake grid ignores llTeleportAgent and llTeleportAgentGlobalCoords
topic: server
status: done
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [server-lsl-lib-avatar-control, server-lsl-lib-experience,
  test-lsl-differential-corpus-triage, server-fake-grid-telehub-routing,
  server-lsl-opensim-teleport-by-region-name]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`RegionHost::teleport_agent` (`sl-fake-grid/src/scripts.rs`) refuses every
scripted teleport as unmodelled, so `experience/powers` never moves an avatar
on either fake flavour. OpenSim lands the owner and a granting non-owner at the
aim. Second Life moves the owner alone, and routes it through the parcel's
landing point or telehub (`book/src/gridspec/lsl.md`, the measurements of
[[server-lsl-lib-experience]]). [[server-lsl-lib-avatar-control]] is done and
says a scripted teleport goes through the ordinary teleport phases, which the
fake grid's host does not do.

## Fix

Drive the grid's teleport machinery (`crate::teleport`, as
`FakeGrid::teleport_agent` uses it) from a script's teleport: a local
destination, a landmark, global coordinates and home, with Second Life's
landing point routing. `experience/powers` then agrees on both flavours, and
its entries in `probes/accepted.toml` go.

## Done (2026-11-10)

The host queues a scripted teleport as a delivery, and after the tick
`teleport::scripted` resolves it and runs `teleport_session` on a task of its
own: a teleport waits on the client and, for what the avatar wears, on the
region's next tick, so the tick cannot wait for it. A place in the region is a
location teleport, global coordinates one too (`VIA_GLOBAL_COORDS` on Second
Life, from a new `TeleportPolicy::scripted_global_flags`), a landmark and home
run as the client's own requests of those kinds (their resolution is now shared
with the responder).

Second Life's flavour also routes any teleport but a lure into a parcel with a
landing point, owned by somebody else, onto the landing point, and fails a
local one it would bring no closer with `Could not teleport closer to
destination` (`routes_to_landing_point`, OpenSim's
`CheckLocalTPLandingPoint` rule); the OpenSim flavour allows direct teleport.
The first teleport after a grant alerts the avatar `You have been teleported by
the object '…' on the parcel '…' owned by …`, as aditi did at steps 3 and 10
and not at 4.

The runtime now complains on Second Life when a script holding no
`PERMISSION_TELEPORT` teleports, and follows OpenSim's own rules there: no
seated avatar, `llTeleportAgentGlobalCoords` the owner's alone, and without the
grant the owner wearing the object or anyone on the object owner's land.

Both fake flavours were re-run. fake-opensim agrees with YEngine and its entry
is gone. fake-sl agrees with aditi but for where the owner lands: Mauve routes
to its telehub, which the fake grid does not keep, so that entry stays under
[[server-fake-grid-telehub-routing]]. OpenSim's region-name destination is
[[server-lsl-opensim-teleport-by-region-name]]. Tests in
`sl-fake-grid/tests/scripts.rs` hold the flags, the landing, the alert once per
grant, global coordinates and the landing-point routing on both flavours.
