---
id: viewer-region-name-connecting-after-crossing
title: Top-bar region name stuck on "Connecting..." after crossing into a region never teleported to
topic: viewer
status: bugs
origin: user report while live-testing the teleport-handover fixes on local OpenSim (2026-08-07)
---

Context: [context/viewer.md](../context/viewer.md).

After **crossing** (walking) into a neighbouring region, the top-bar region
name shows **"Connecting..."** instead of the region name, for *every* region
except the login region ("East Region"), to which it correctly reverts when you
walk back. The avatar itself crosses and moves normally — this is purely the
region-**name** label (`status-bar-connecting`, `status_bar.rs:685`, shown when
`SlRegionIdentity.sim_name` is `None`). The event queue is healthy (the crossing
delivers `CrossedRegion` and rebuilds the poller cleanly).

Root cause: the region name comes from a `RegionHandshake`
(`Event::RegionInfoHandshake` → `SlRegionIdentity`), and the viewer attaches
that identity to **whichever region is current at the time**
(`world.rs:305-310`, `current_entity(&index)`). The login region gets its
handshake while it is current, so it is labelled forever. A region entered by a
**crossing never has a `RegionHandshake` attributed to it**, so its identity
stays empty and the label falls back to "Connecting...".

The open question that picks the fix: **does OpenSim / SL send a
`RegionHandshake` for a region entered by crossing (or on the child circuit when
the neighbour is first enabled)?** The session already decodes `RegionHandshake`
on both the root and child circuits (`methods.rs:1770` and `methods.rs:2525`,
both emit `RegionInfoHandshake`), but `RegionInfoHandshake` carries no source
region handle, so the viewer can only guess "current".

Candidate fixes:

- If the sim **does** send `RegionHandshake` on child circuits / on the
  crossing: carry the source region handle on `Event::RegionInfoHandshake` and
  attach `SlRegionIdentity` to *that* region's entity (by handle), not the
  current one — so each neighbour's name is cached and a crossing shows it
  immediately. (This also fixes a latent misattribution: a neighbour's
  handshake arriving while another region is current would otherwise overwrite
  the current region's identity.)
- If the sim does **not**: resolve the current region's name from the map / grid
  service by handle (the world-map already fetches region names by handle) as a
  fallback when `sim_name` is `None`.

Diagnose: log every received `RegionHandshake` with its circuit/region on the
running instrumented build, cross a border, and see whether a handshake arrives
for the destination (and on which circuit) — that decides which fix applies.
