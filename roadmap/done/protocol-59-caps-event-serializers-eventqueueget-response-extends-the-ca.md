---
id: protocol-59
title: CAPS event serializers + EventQueueGet response (extends the CAPS
topic: protocol
status: done
origin: ROADMAP.md — Tier F
---

Context: [context/protocol.md](../context/protocol.md).

**59. CAPS event serializers + `EventQueueGet` response (extends the CAPS
items #10/#13/#28/#30). ✅ Done.** For each inbound CAPS parser in `session.rs`
the inverse `*_to_llsd` was added — the element-by-element mirror that lets a
simulator / grid service *produce* the LLSD body the client decodes, so an
`Llsd` round-trips back to an equal decoded value: `teleport_finish_to_llsd`,
`enable_simulator_to_caps_llsd`, `crossed_region_to_caps_llsd`,
`establish_agent_communication_to_llsd`, `server_appearance_update_to_llsd`,
`parcel_info_to_llsd`, `offline_messages_to_llsd`,
`chatterbox_invitation_to_llsd`, `group_memberships_to_caps_llsd`,
`group_members_to_caps_llsd`, `inventory_descendents_to_llsd`,
`bulk_update_inventory_to_llsd`, `ais_inventory_update_to_llsd` and
`created_category_to_llsd` (with `pub(crate)` folder/item/record leaf helpers
`inventory_folder_to_llsd` / `inventory_item_to_llsd` /
`bulk_update_item_to_llsd` / `offline_message_to_record`). Plus
`build_event_queue_response(id, &[EventQueueEvent])` in `sl-wire/src/llsd.rs` —
a `{ id, events: [{ message, body }…] }` batch built on #52's
`Llsd::to_llsd_xml`, the inverse of `parse_event_queue_response` and the server
counterpart of the client's `build_event_queue_request`. The top-level encoders
are exported `pub` from `sl-proto` (terrain-style: no runtime consumer yet,
reused by the `SimSession` skeleton, #60). New `u32`/`u64` LLSD encoders mirror
the tolerant `llsd_u32`/`llsd_u64` readers (plain integer when it fits an `i32`,
else big-endian binary — the `big_endian_bytes` lint forbids `to_be_bytes`, so
the bytes are extracted by hand); `ParcelRequestResult`/`ParcelStatus` gained
`to_i32` and `LandingType` a `to_u8` (the inverse classifiers). Covered by 14
`sl-proto` round-trip tests + 1 `sl-wire` test (each value → `*_to_llsd` →
`*_from_llsd` → equal; AIS uses uuid-keyed unordered maps so its test sorts
before comparing). Built on #52.
