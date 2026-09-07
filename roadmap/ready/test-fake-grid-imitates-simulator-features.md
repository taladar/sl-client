---
id: test-fake-grid-imitates-simulator-features
title: A grid claiming to be Second Life still introduces itself as OpenSim
topic: test
status: ready
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 3
refs: [test-fake-grid-object-asset-id-divergence]
---

Context: [context/testing.md](../context/testing.md).

How a region describes *itself* is the third divergence `ImitatedGrid` does not
decide, and the fake grid takes OpenSim's side of all of it:

- **`OpenSimExtras`** rides in `SimulatorFeatures` unconditionally
  (`runtime.rs`), carrying the map-tile server URL and the currency helper
  base. Second Life sends no such block — a viewer discovers those elsewhere —
  so a viewer that learned to read its map server *only* out of the extras
  works here and against OpenSim, and finds nothing on Second Life.
- **Voice** is always the WebRTC stub. That half is right for Second Life and
  wrong for OpenSim, which is Vivox or nothing — so this is the one place the
  grid is accidentally Second Life while being OpenSim about everything else in
  the same message.

The awkward part is that the map and currency URLs have to keep working on a
Second-Life-flavoured grid whether or not the extras block carries them: the
fake grid *is* its own tile server and its own currency helper, and a flavour
that hid the URLs without providing the other route would break the world map
offline. So the work is to find what Second Life's viewer path actually reads
for each and serve that, not merely to drop a field.

One thing deliberately stays put: `GridIdentity::platform` remains `OpenSim`
whichever grid is being imitated. It is not protocol behaviour — it is what
Firestorm's grid manager reads to decide whether it will add the grid at all,
and a fake grid Firestorm refuses to add tests nothing. That is stated in
`imitates.rs` and should stay stated.

Acceptance: `SimulatorFeatures` and the login response describe the grid the
flavour names — no `OpenSimExtras` on the Second Life side, no WebRTC voice on
the OpenSim one — with the map and currency surfaces still reachable on both,
and a conformance case that reads them declares the flavour it expects.
