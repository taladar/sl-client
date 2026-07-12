---
id: idiomatic-p5-08
title: Union keys (kept client-local, not sl-types)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Union keys (kept client-local, *not* `sl-types`).** Four discriminated-
union UUID fields whose referent is one-of-several key kinds. **Scope change
(user, this session, via AskUserQuestion):** the new unions were kept
**client-only in `sl-proto`**, NOT added to `sl-types` — the general ones
(`AgentOrObjectKey`, `InventoryItemOrFolderKey`) may move to `sl-types` later
bundled with other changes to avoid a `sl-types` release churn; the wire-codec
ones (`MeshKey`, `SculptOrMeshKey`) are client details. **NO `sl-types` change
at all.** New `sl-proto/src/types/union_key.rs`: `MeshKey(pub Uuid)`,
`SculptOrMeshKey { Sculpt(TextureKey), Mesh(MeshKey) }`,
`AgentOrObjectKey { Agent(AgentKey), Object(ObjectKey) }`,
`InventoryItemOrFolderKey { Item(InventoryKey), Folder(InventoryFolderKey) }`
(each with `uuid()`/`is_*`/`Display`, modelled on `OwnerKey`). Per-field:
(1) **`SculptData.texture`** (`types/object.rs`) `Uuid` → `SculptOrMeshKey`,
discriminated by the low bits of `sculpt_type` (`== LL_SCULPT_TYPE_MESH` →
mesh asset, else sculpt texture) in `extra_params.rs` decode/encode; wire
byte-identical. (2) **`InventoryOffer.item_id`** (`types/chat.rs`) `Uuid` →
`InventoryItemOrFolderKey`, discriminated by `asset_type == AssetType::Folder`
in the IM-bucket decode (and the REPL builder). (3) **`ChatMessage`** — the
`source_id: Uuid` + `source_type: ChatSourceType` pair FOLDED into one
`source: ChatSource { System, Agent(AgentKey), Object(ObjectKey), Unknown {
source_type, source_id } }` (with `from_wire`/`source_id`/`source_type_byte`/
`agent_or_object()→Option<AgentOrObjectKey>`); the server encoder
`SimSession::send_chat_from_simulator` now takes `ChatSource` (one fewer arg).
Round-trip lossless (Unknown preserves both bytes; System ⇒ nil id).
(4) **`DerezObjects.destination_id`** — FOLDED the id INTO the
`DeRezDestination` variants (user picked this over a union+Option):
`SaveIntoAgentInventory(InventoryKey)`/`AcquireToAgentInventory`/`Take…`/
`ForceToGod…`/`Trash(InventoryFolderKey)` carry a folder/item,
`SaveIntoTaskInventory(ObjectKey)` a task, the rest none; new
`DeRezDestination::destination_id() -> Uuid` (nil for the id-less ones) feeds
the wire, and `Command::DerezObjects`/`Session::derez_objects` dropped their
`destination_id` field/param. Semantics verified against Firestorm
`derez_objects` call sites + OpenSim `DeRezAction`. Re-exported all five new
types through `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL
derez grammar keeps `<destination> <destination_id>` and recombines.
+4 focused unit tests (union round-trips; sculpt mesh-vs-texture
discriminator; folder-vs-item offer; `ChatSource` wire round-trip;
`DeRezDestination` code+id). Build + clippy (`--workspace --all-targets`) +
699 tests + `cargo doc` (`-D warnings`) + mdbook green. **NO `sl-types`
change.**
