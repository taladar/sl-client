---
id: idiomatic-p4-02
title: LocalId(u32) — public in types/object.rs:60, types/editing.rs:587; re-
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 4 — Domain ID newtypes (medium-high invasiveness)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`LocalId(u32)` — public in `types/object.rs:60`, `types/editing.rs:587`;
re-key `objects: BTreeMap<_, BTreeMap<u32, Object>>` (`session.rs:818`);
reconcile the parcel `local_id: i32` inconsistency (`types/parcel.rs:170`).
**Renamed (user-approved) for a self-describing, symmetric pair** — the bare
`LocalId` was under-descriptive, so two public newtypes live in
`sl-wire/src/region_local_id.rs` (mirroring `RegionHandle`/`sl-types` key
ergonomics: `Copy`/`Eq`/`Hash`/`Ord`/`Default`, `new`/`get`, `Display`):
`RegionLocalObjectId(pub u32)` (the object id, `LLViewerObject::mLocalID`) and
`RegionLocalParcelId(pub i32)` (the parcel id, `LLParcel::mLocalID`). Keeping
them as **distinct types with the wire's own signedness** (`u32` vs `i32`) is
what reconciles the historical `local_id: u32` / `local_id: i32`
inconsistency, and makes "passed a parcel id where an object id was expected"
(and vice-versa) a compile error. Maximal scope: replaced **every** object
region-local id with `RegionLocalObjectId` — `Object.local_id`/`parent_id`,
`TerseUpdate`, `ObjectBuyItem`, `GltfMaterialOverride` (sl-wire), the object
cache's inner `BTreeMap` key, `task_local_id`, the `object_local_id` telehub
fields, `parse_object_physics_properties`'s `(RegionLocalObjectId, _)` pairs,
the `Event`/`ServerEvent`/`Command` object-id fields **including the plural
`Vec<u32>`/`&[u32]` lists** (delete/link/delink/select/deselect/duplicate/
detach/drop/derez/…), and the public `Session`/`SimSession` methods that take
them — and **every** parcel region-local id with `RegionLocalParcelId`
(`ParcelInfo`, `ParcelVoiceInfo` + `VoiceProvisionRequest` (sl-wire),
`ParcelScriptResources` (sl-wire), the parcel-management `Command`/`Event`/
`ServerEvent` fields, and all the `request_parcel_*`/`reclaim_parcel`/
`release_parcel`/`buy_parcel_pass`/… method params). Codec wraps at the
boundary (decode `RegionLocal*Id(raw)`, encode `.0`) so wire bytes are
byte-identical; the *sequence-number* ack arrays (`record_acks`) and the
terrain DCT `decopy: &[u32]` were left raw (not ids). Re-exported through
`sl-proto`/`sl-client-tokio`/`sl-client-bevy` (both runtimes at parity); REPL
arg parsers parse the raw int then wrap, survey unwraps `.0` for its raw-int
JSON record. +4 unit tests on the newtypes (raw round-trip incl. the negative
parcel sentinel, `Display`, the `0` object sentinel); lifecycle +
`sim_session` round-trip suites updated. NO sl-types touched (client concepts
in `sl-wire`).
