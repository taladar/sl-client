---
id: idiomatic-p5-03
title: OwnerKey sweep (owner_id — agent-or-group)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`OwnerKey` sweep (`owner_id` — agent-or-group). Replaced the raw agent-or-group
owner fields with `sl_types::key::OwnerKey`
**only where the wire actually expresses the union** (a discriminator is
present); discriminator-less owner ids and **every `last_owner_id`** stay raw
`Uuid` (no agent/group tag on the wire — same precedent as the GroupKey-sweep
dialog-discriminated IM field). NO sl-types change (OwnerKey already had
`Copy`/`Hash`/`uuid()`/`From<Uuid>`/ `is_group()` from the AgentKey sweep's
0.4.0). Two wire shapes, both collapsed to
**one `OwnerKey` field ⇄ the two wire fields on encode** (user-directed — no
double storage). **Type X — explicit `*_is_group` bool, the id itself holds the
group when set** (Firestorm: parcel
`mGroupOwned // true if mOwnerID is a group_id`):
`MoneyTransaction.source`/`dest` (`is_source_group`/`is_dest_group`),
`ParcelInfo.owner`, `ParcelObjectOwner.owner`, `LoadUrlRequest.owner`, and the
sl-wire `ScriptedObjectInfo.owner` (`is_group_owned`) — decode
`owner_key_from_wire(id, flag)`, encode `owner.uuid()`/`owner.is_group()`, no
group slot, zero redundancy. **Type Y — group-owned signals via a *null*
`OwnerID` with the owning group in a separate `GroupID`** (objects via the
null-convention, inventory via an explicit `GroupOwned` flag):
`ObjectProperties`, `ObjectPropertiesFamily`, `InventoryItem`, `RestoreItem` —
`owner: OwnerKey` (the `Group` variant sourced from `GroupID`) plus the separate
set-to group **`group: GroupKey` → `group: Option<GroupKey>`** (`None` = no
group set, killing the `GroupKey(nil)` footgun — user-requested), the
now-redundant `group_owned` bool removed. Codec helpers in
`sl-proto/src/types.rs`
(`object_owner_from_wire`/`inventory_owner_from_wire`/`object_owner_to_wire`/
`group_from_wire`/`group_to_wire`) keep the wire bytes byte-identical, incl. the
`inventory_item_crc` checksum (recomputed from the wire `(OwnerID, GroupID)`
pair). `ScriptedObjectInfo` lost its `Default` derive → equivalent manual impl.
Re-exported `OwnerKey` through `sl-proto`/`sl-client-tokio`/`sl-client-bevy`
(parity); REPL `build_inventory_item`/`RezRestoreToWorld` keep their keyword
grammar and recombine into `owner`/`group`, survey unwraps `owner.uuid()`/
`owner.is_group()` for its raw JSON record. +4 focused round-trip unit tests
(`owner_codec_tests`) covering both shapes incl. the group-owned null path;
lifecycle + `sim_session` suites updated. **Left for later families
(deliberately, no discriminator / different family):** `Object.owner_id` (live
ObjectUpdate, sound-only, no tag), `ScriptDialog.owner_id`,
`SoundPreload.owner_id`, `RezAttachment.owner_id`, `ChatMessage.owner_id`,
`DirEventResult`/`PlacesResult`/ `ParcelDetails.owner_id`,
`GroupNoticeAttachment.owner_id`, `estate_owner_id` (an agent → AgentKey
family), and `BuyParcel`'s `group_id`+`is_group_owned` (buyer intent, not an
owner field).
