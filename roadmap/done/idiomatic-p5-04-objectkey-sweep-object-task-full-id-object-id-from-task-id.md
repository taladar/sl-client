---
id: idiomatic-p5-04
title: ObjectKey sweep (object/task/full_id/object_id/from_task_id)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`ObjectKey` sweep (object/task/`full_id`/`object_id`/`from_task_id`).
Replaced every raw `Uuid` field that is unambiguously an **in-world object /
task** (LL's `mFullID`/`ObjectID`/`TaskID`) with `sl_types::key::ObjectKey`,
wrapping at the codec boundary only (wire bytes byte-identical). NO sl-types
change needed — `ObjectKey` already had `Copy`/`Hash`/`uuid()`/`From<Uuid>`
from the AgentKey sweep's 0.4.0. Converted carriers: `Object.full_id`,
`ObjectProperties`/`ObjectPropertiesFamily` `object_id` + `from_task_id`
(the `from_task_id` "task" = the object an item was rezzed out of),
`ParticleSystem.target_id` (documented "target object"), `ScriptDialog`/
`LoadUrlRequest` `object_id` and `ScriptPermissionRequest.task_id`,
`NotecardRez` `from_task_id`/`ray_target_id`/`object_id`,
`AvatarAnimationSource.source_id` (`Option<ObjectKey>`),
`SoundPreload.object_id`,
`AvatarAttachment.id`, `LandStatItem.task_id`, `TelehubInfo.object_id`, the
`ViewerEffectData` `LookAt`/`PointAt` `target` and `Spiral` `source`/`target`
(explicitly deferred to here by the AgentKey sweep), every object-bearing
`Event` (`SetFollowCamProperties`/`ClearFollowCamProperties`/`SitResult`
`sit_object`/`PayPriceReply`/`ScriptRunning`/`ObjectMedia`/`SoundTrigger`
`object_id`+`parent_id`/`AttachedSound`/`AttachedSoundGainChange`) and the
cost/physics reply keys (`Event::ObjectCosts`/`ObjectPhysicsData` →
`Vec<(ObjectKey, _)>`), `Command`/`ServerEvent` object fields (script
dialog/permissions, cost/physics/selected-cost `object_ids: Vec<ObjectKey>`,
parcel return/select/disable `task_ids`/`object_ids`, grab-update,
buy-inventory, pay-price, properties-family, spin, duplicate-on-ray
`ray_target_id`, the three script-running variants, the three object-media
variants), the `AbuseReport`/`ObjectMediaResponse`/`MaterialOverrideUpdate`
sl-wire structs, and the sl-wire
`build_get_object_cost_request`/`parse_get_object_cost`/`…_response`/
`build_resource_cost_selected_request`/`parse_resource_cost_selected_request`/
`build_get_object_physics_data_*`/`build_object_media_*_request` helper
signatures. The ~40 `Session`/`SimSession`/circuit-sender method params that
take a persistent object id (not a region-local id) now take `ObjectKey`.
**Left raw (deliberately):** `ChatMessage.source_id` (agent-*or*-object union,
discriminated by `source_type` → deferred to the union-key item),
`DerezObjects.destination_id` (folder-*or*-task union), and the non-object
families already named for later sweeps (`owner_id`/`last_owner_id`,
inventory `item_id`/`folder_id`, `texture_id`/`sound`/`asset_id`, `parcel_id`,
agent/group ids). REPL gains `req_object`/`object_or_nil`/`vec_object` arg
helpers (parse the raw UUID then wrap); `SessionContext.last_object` is now
`Option<ObjectKey>`; both runtimes' object-media fetch take `ObjectKey`
(parity). Book `content/region.md` updated. +1 focused unit test
(`object_key_round_trips_raw_uuid`: wrap/unwrap is the identity, default
`full_id` is the nil object key); lifecycle + `sim_session` round-trip suites
updated. NO sl-types touched.
