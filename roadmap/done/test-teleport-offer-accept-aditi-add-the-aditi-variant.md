---
id: test-teleport-offer-accept-aditi
title: Teleport offer/accept — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `teleport-offer-accept` case
(`[[test-teleport-offer-accept]]`, already green `[opensim]`): **green on
aditi live** (2026-08-12, Phase Z batch) after fixing a real client bug
the run exposed.

**The arrival region handle after a lure teleport was garbage on Second
Life.** The session seeded the handover's region handle from
`parse_lure_region_handle` — reading the first 8 bytes of the lure id as a
handle, which is an **OpenSim convention**; SL's lure id is opaque, so the
requested-target handle (and hence `Event::RegionChanged.region_handle`
and `Session::region_handle()`) was a random u64 after an accepted offer.
Fix in `sl-proto/src/session/methods.rs`: `commit_handover` now takes the
destination simulator's own `AgentMovementComplete` `Data.RegionHandle`
(the authoritative arrival statement, previously ignored) and prefers it
(when non-zero) over the pending handover's requested handle for both the
regions map and the `RegionChanged` event. OpenSim is unaffected (its two
sources agree). Residual known limit: the adjacency/world-reset
classification still runs at request time on the guessed handle, so an SL
lure classifies as a world reset — the safe direction (purge + refetch).
