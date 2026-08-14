---
id: protocol-sim-caps-content
title: Server-side content upload/update caps, materials and MOAP
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 8
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The upload/update flows, server side:

- `NewFileAgentInventory`, `UploadBakedTexture`,
  `UpdateAvatarAppearance`;
- the `Update{Gesture,Notecard,Script,Settings,Material}` ×
  `{Agent,Task}Inventory` caps — the two-stage upload flow (grant an
  uploader URL → receive the body → return the new asset id) as a
  reusable server-side state machine;
- `RenderMaterials` / `ModifyMaterialParams` /
  `UpdateMaterialAgentInventory`;
- `ObjectMedia` / `ObjectMediaNavigate` (MOAP).

Inverse-pairing per the convention; the two-stage uploader is verified by
driving the client `Session`'s own upload flow against it in-memory.

Done (2026-08-14): fifteen `REQUESTED_CAPABILITIES` rows flipped to Served in
the pinned coverage table (`ObjectAnimation` stays Pending — it is never POSTed,
only opting into the UDP stream). Seven new `CapHandler` variants in `SimCaps`
(`sl-proto/src/sim_caps.rs`): one **`AssetUpload`** serving the whole two-stage
family (`NewFileAgentInventory`, `UploadBakedTexture`, every
`Update{Gesture,Notecard,Script,Settings,Material}{Agent,Task}Inventory`) as a
reusable state machine — step 1 parks the parsed metadata + mints an `upload`
sub-path uploader URL, step 2 mints ids from a monotonic per-session serial
(`SimSession::next_sim_serial`, a documented sim simplification) and pushes
`ServerEvent::CapsAssetUploaded`; plus `AvatarAppearance`,
`CopyInventoryFromNotecard`, `RenderMaterials` (POST/GET query + PUT set),
`ModifyMaterialParams`, `ObjectMedia` (GET/UPDATE verbs) and
`ObjectMediaNavigate`. Materials/MOAP reads serve from new driver-populated
`SimSession` stores (`region_materials`, `object_media`) — the world authority
stays out of scope, so mutations surface as `ServerEvent`s
(`RenderMaterialsSet`, `MaterialParamsModified`, `ObjectMediaSet`,
`ObjectMediaNavigated`, `ServerAppearanceRequested`,
`CopyInventoryFromNotecardRequested`). New sl-wire inverses beside their client
partners: `parse_new_file_agent_inventory_request`,
`parse_update_item_asset_request`, `parse_update_task_item_asset_request`,
`parse_update_script_{agent,task}_request`,
`parse_update_avatar_appearance_request`, `parse_object_media_request` /
`parse_object_media_navigate_request` / `ObjectMediaResponse::to_llsd`,
`parse_render_materials_request` / `parse_render_materials_put_request` /
`build_modify_material_params_response`, plus
`parse_copy_inventory_from_notecard` in `sl-proto`. Verified by 11 loopback
tests driving the real client builders/parsers against `SimCaps::dispatch`
(`sl-proto/tests/sim_caps.rs`) and sl-wire codec round-trips; book coverage is
the new "content upload & media handlers" subsection of
`book/src/comms/caps.md`.
