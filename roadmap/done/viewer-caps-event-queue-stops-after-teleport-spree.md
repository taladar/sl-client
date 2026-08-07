---
id: viewer-caps-event-queue-stops-after-teleport-spree
title: CAPS EventQueueGet stops delivering (no CrossedRegion) after a rapid teleport spree, freezing the avatar at a crossing
topic: viewer
status: done
origin: user report while live-testing the teleport-handover fixes on local OpenSim (2026-08-07)
refs: [viewer-crossing-movement-locks-up, protocol-simfeatures-503]
---

Context: [context/viewer.md](../context/viewer.md).

After a **rapid teleport spree** (six teleports in ~8 s, then a local
teleport), walking into a region **corner** left the avatar playing its walk
animation but **frozen in place**. OpenSim's log shows it crossing the agent
across the corner many times (`CrossAgentToNewRegionAsync … Crossing agent
completed`, bouncing East↔Northeast↔North), but the **viewer received none of
those `CrossedRegion` events** — nothing logged after the last teleport, no
`begin_crossing`, no `RegionChanged`. The client stays `Active` in the region it
last teleported to while the server moves its root region repeatedly.

On OpenSim, `CrossedRegion` (like `EnableSimulator` / `TeleportFinish`) is
delivered over the **CAPS `EventQueueGet` long-poll**, which the bevy driver
restarts on every `RegionChanged` (`sl-client-bevy/src/lib.rs:3869`
`if region_changed { caps = start_caps(&session) }`, `caps.rs::start_caps` off
`session.seed_capability()`). The event queue was **alive through the last
teleport** (it delivered that `TeleportFinish`) and **silent afterward**, so the
final post-teleport restart left no live poll.

Not caused by the 2026-08-07 handover fixes (world_reset adjacency /
abort-drops-child / own-avatar-region gate) — the
`start_caps`-on-`RegionChanged` path is unchanged by them.

Suspected causes (need event-queue-level logging to confirm):

- The post-teleport seed / `EventQueueGet` fetch not coming back live — OpenSim
  CAPS serving is known-flaky here (see [[protocol-simfeatures-503]]); a failed
  seed POST after a teleport yields no `EventQueueGet` URL → no long-poll.
- Rapid `RegionChanged`s across frames spawning/replacing the long-poll thread
  such that the last `start_caps` does not end up with a live poller (old
  `Caps` drop vs new thread start ordering under a burst).
- `session.seed_capability()` being stale for the committed root at the instant
  `start_caps` runs after a deferred-teardown neighbour teleport.

Next steps:

- Add diagnostics to `start_caps` / `run_caps`: log the seed used, whether an
  `EventQueueGet` URL was found, each long-poll round's outcome, and every event
  forwarded — so a repro pinpoints where delivery stops.
- Confirm the trigger: a **clean relog + a single normal border crossing (no
  teleport spree)** should work (crossings worked earlier in the same session);
  the spree (and/or the corner) is what breaks it. Then reintroduce the spree to
  reproduce.
