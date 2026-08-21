---
id: viewer-fake-grid-login-smoke
title: The real client stack against the fake grid
topic: viewer
status: done
origin: user request (2026-07) — full client testing without a real server
points: 5
blocked_by: [viewer-fake-grid]
---

Context: [context/viewer.md](../context/viewer.md).

The only tier exercising the socket-owning `drive` system,
retransmission, and CAPS polling end-to-end: run `SlClientPlugin` in a
headless app pointed at an in-process fake grid.

Assert the full pipeline — login → circuit handshake → `RegionHandshake`
→ `SlEvent` stream → `maintain_world` region/parcel/object state — and a
few command round-trips (chat out → decoded server-side as a
`ServerEvent`; object update in → viewer `ObjectState`).

Deliberately a smoke tier: behaviour lives in the headless
interaction/world tiers; this proves the plumbing those tiers bypass is
sound.

Done (2026-08-21): `sl-client-bevy/tests/fake_grid_login_smoke.rs` logs the
real `SlClientPlugin` (a `MinimalPlugins` app stepped by hand; the grid on
a test-owned tokio runtime) into `sl-fake-grid` and asserts the pipeline
in order — login → `CircuitEstablished` → `RegionHandshakeComplete` →
`SlIdentity`; `maintain_world` state (the current `SlRegion` with its
`SlRegionIdentity`, a complete `SlParcelOverlay`, the stock parcel as
`SlAgentParcel.current` and as the region's `SlParcel` child); the stock
greeting / prim / `SimulatorFeatures` over the Bevy CAPS path / a seed
grant with `EventQueueGet`; a chat `SlCommand` decoded grid-side; an
`ObjectUpdate` pushed via `with_sim` arriving as `ObjectAdded` and a
`KillObject` as `ObjectRemoved`; a CAPS `ParcelProperties` through the
real long-poll renaming the agent parcel; a clean `LoggedOut` — plus two
apps against two grids in one process.

Making that assertable needed the grid side to push what a real
simulator pushes on region entry: `SimSession` gained the typed senders
`send_parcel_properties` / `enqueue_parcel_properties` (UDP + CAPS event
queue, inverses of the client decoders, field-for-field round-trip
tests), `send_parcel_overlay(_chunk)`, `send_object_update`,
`send_object_update_compressed`, `send_kill_object`, and decodes
`ParcelPropertiesRequest` → `ServerEvent::RequestParcelProperties` and
`RequestMultipleObjects` → `ServerEvent::RequestObjects`. `sl-fake-grid`
gained `Scenario::world` (`SceneFixtures`: parcels + objects, the agent's
own avatar object, the overlay derived from the parcels) pushed on
`AgentArrived` and replayed on request; the stock world is the
region-wide parcel's record and the scripted object as a visible box.
Bug found by the tier: the driver sent `RegionHandshake` on `AgentArrived`
(after `AgentMovementComplete`), which the client drops once its arrival
is complete — it now goes out on `CircuitOpened`, as a real sim does; the
tokio e2e now pins `RegionInfoHandshake` too. Still open from the
fake-grid series: a `teleport_agent` helper, the viewer crate's
`update_objects` against the fixtures (`viewer-world-test-harness`), and
pointing Firestorm at the binary.
