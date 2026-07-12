---
id: idiomatic-p4-03
title: Circuit-scoped id wrappers for the user-facing API.** Added two public
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 4 — Domain ID newtypes (medium-high invasiveness)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Circuit-scoped id wrappers for the user-facing API.** Added two public
scoped-id structs in `sl-proto/src/scoped_id.rs`:
`ScopedObjectId { circuit: CircuitId, id: RegionLocalObjectId }` and
`ScopedParcelId { circuit, id: RegionLocalParcelId }`, plus a new opaque
**`CircuitId(u64)`** that is the chosen circuit key (user-approved over
`SocketAddr`/`RegionHandle`). `CircuitId` is a per-establishment **instance
token** minted from a monotonic `Session` counter every time a circuit is
established (root at login, each child at `EnableSimulator`, a fresh root on a
teleport `retarget`); a child promoted to root across a border **keeps** its
id (same connection). It is deliberately *not* derived from address/region, so
a reconnect to the same address/region mints a *different* `CircuitId` and a
stale scoped id fails to resolve — capturing the session/connection scope the
user correctly identified (a region-local id is only reliably valid for the
lifetime of the one circuit it was learned on). The four per-circuit caches
(`objects`/`terrain`/`regions`/`time_dilation`) were **re-keyed from
`SocketAddr` to `CircuitId`** (user-approved), so the address-reuse-after-
reconnect hazard is structurally impossible. The wire codec still encodes only
the bare `RegionLocalObjectId`/`RegionLocalParcelId` (the scope is never
serialized). Surfacing: the `Object` struct gained a `circuit: CircuitId`
field (stamped at cache `upsert`) + `Object::scoped_id()`/`scoped_parent_id()`
accessors; the id-bearing `Event`s now carry the scoped form (`ObjectRemoved`/
`GltfMaterialOverride`/`ObjectPhysicsProperties`/`ParcelDwell`/
`ParcelAccessList`), and `Event::CircuitEstablished`/`RegionChanged` gained a
`circuit: CircuitId` so a caller can track the current circuit. Consuming: the
~44 object/parcel `Session` methods and `Session::object` take the scoped form
and resolve it via `circuit_for_scope` (→ `Error::NoCircuit` if not logged in,
new `Error::UnknownCircuit` if the circuit is gone/stale; new
`Error::MixedCircuits` for a batch slice spanning circuits); the matching
`Command` enum fields are scoped too, with the runtimes forwarding verbatim.
New `Session::root_circuit_id()` lets a driver build a scoped id from a raw
id. Re-exported through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`
(parity); the
REPL `SessionContext` tracks the current circuit (`$circuitid`, fed from the
two circuit events) and `registry.rs` scopes freshly typed ids via
`scoped_object`/`scoped_parcel`/`scoped_objects(ctx, …)`; examples use
`Object::scoped_id()`. Book `content/world.md` documents the scoping. +5
scoped-id unit tests and a focused lifecycle test
(`scoped_object_id_is_circuit_bound`: the right circuit resolves and sends, a
foreign/stale circuit returns `None` / `Error::UnknownCircuit`); lifecycle +
`sim_session` suites updated. NO sl-types touched (client concepts in
`sl-proto`/`sl-wire`).

Then internal bookkeeping IDs (lower misuse surface, do last):
