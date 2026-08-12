---
id: viewer-region-name-connecting-after-crossing
title: Top-bar region name stuck on "Connecting..." after crossing into a region never teleported to
topic: viewer
status: done
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

## Done (2026-08-12)

Candidate fix #1 (attribute the identity by handle). The diagnosis's premise —
that `RegionInfoHandshake` "carries no source region handle" — was stale:
`RegionIdentity` already carries `region_handle`, and both handshake handlers
resolve it from the receiving circuit (`methods.rs` root ~2606, child ~1771, via
`self.regions.get(&circuit_id)`). So `maintain_world`
(`sl-client-bevy/src/world.rs`) now attaches `SlRegionIdentity` to the entity
for `region_identity.region_handle` (new `entity_for_handle`), falling back to
the current region only when the handle is `0` (never learned). This also
removes the latent misattribution where a neighbour's handshake overwrote the
current region's identity.

Diagnose answer (settled live): **OpenSim *does* send a `RegionHandshake` for
every neighbour on its child circuit, right after login, before any crossing.**
A single Default-Region login logged all four regions' identities attributed to
their own entities (`attributed=true`): Default (current) + East + North +
Northeast. So each neighbour's name is cached ahead of time and a crossing shows
it immediately — no grid-service fallback (candidate #2) was needed. The child
handshake fires because `EnableSimulator` (UDP or the CAPS event-queue form on
OpenSim) opens the child circuit, which draws the neighbour's `RegionHandshake`.

Live-verified on local OpenSim (2×2 region grid) across multiple crossings: the
top-bar region name now reads the destination region's name immediately instead
of "Connecting…", and reverts correctly when walking back. Unit tests in
`world.rs` cover both the per-region caching (a neighbour handshake lands on the
neighbour, not the current region, and a crossing then reads it) and the
handle-`0` fallback to the current region. A
`debug!("region handshake identity")` in the handler records the attribution for
any future regression.
