---
id: protocol-36
title: ObjectProperties full field surface (extends #17, Tier C). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**36. `ObjectProperties` full field surface (extends #17, Tier C). ✅ Done.**
The `ObjectProperties` struct (`types.rs`) ended at `sit_name`; the decoder
`object_properties` (`session.rs`) dropped 8 wire fields of the `ObjectData`
block. Added and populated all of them: **`ItemID`** (`item_id` — correlate
an in-world object back to the inventory item it was rezzed from, needed for
attachments and "find in inventory"), `FolderID` (`folder_id`), `FromTaskID`
(`from_task_id` — the source object when rezzed from another object's
contents), `InventorySerial` (`inventory_serial`, an `i16` that bumps on
task-inventory changes so a client can detect them without re-fetching), the
three aggregate-permission rollups
(`aggregate_perms`/`aggregate_perm_textures`/`aggregate_perm_textures_owner` —
the build-floater "next owner can…" summary), and `TextureID`, surfaced as a
structured **`texture_ids: Vec<Uuid>`** by splitting the wire blob into
back-to-back 16-byte UUIDs (a new `concatenated_uuids` helper, ignoring any
trailing partial id). The struct is re-exported through both runtimes (no
destructuring sites needed changes — every consumer binds the whole
`ObjectProperties`). Covered by the extended
`object_properties_surface_and_merge` `sl-proto` lifecycle test (asserts the
serial, the three source ids, the three aggregate-perm bytes, and a two-UUID
`texture_ids` decode). *Live-verified against the local OpenSim via the
`rez_edit_object` example: an `ObjectSelect` → `ObjectProperties` round-trip
on a freshly-rezzed cube decoded all eight new fields end-to-end with no
protocol error — nil source ids, serial 0, zero aggregate perms and an empty
`texture_ids` (faithful: a fresh `ObjectAdd` prim has no source-inventory item
and OpenSim sends no texture-id blob in its `ObjectProperties`); the
non-trivial values are covered by the unit test. Test: local OpenSim.*
