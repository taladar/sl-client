---
id: idiomatic-p5-06
title: TextureKey sweep (texture/asset image ids)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`TextureKey` sweep (texture/asset image ids). Replaced every raw `Uuid`
    field that is unambiguously a **texture/image asset** with
    `sl_types::key::TextureKey`, wrapping at the codec boundary only (decode
    `TextureKey::from(..)`, encode `.uuid()`) so the wire bytes are
    byte-identical. NO sl-types change — `TextureKey` already carries
    `Copy`/`Hash`/`uuid()`/`From<Uuid>`/`Display` from the AgentKey-sweep
    0.4.0, so sl-types stayed clean (no version bump). Converted carriers:
    avatar profile imagery (`AvatarProperties`/`ProfileUpdate`
    `image_id`+`fl_image_id`,
    `PickInfo`/`PickUpdate`/`ClassifiedInfo`/`ClassifiedUpdate` `snapshot_id`,
    `AvatarGroupMembership.group_insignia_id`); parcel media/snapshot
    (`ParcelInfo`/`ParcelUpdate` `media_id`+`snapshot_id`,
    `ParcelMediaUpdateInfo.media_id`, the directory-land result
    `snapshot_id`); group insignia (`GroupMembership.group_insignia_id`,
    `GroupProfile`/`CreateGroupParams` `insignia_id`); object surface/light
    textures (`LightImage.texture` projected light,
    `ParticleSystem.texture_id`,
    `ObjectProperties.texture_ids: Vec<TextureKey>`, `TextureFace.texture_id`
    + the `TextureEntry::texture_id()` accessor → `Option<TextureKey>`);
    directory (`PlacesResult.snapshot_id`); map tiles
    (`MapRegionInfo.map_image_id`, `MapLayer.image_id`); script dialog icon
    (`ScriptDialog.image_id`); EEP environment (`SkySettings`
    sun/moon/cloud/bloom/halo/rainbow textures, `WaterSettings`
    `normal_map`+`transparent_texture`); the fetched `Texture.id`; the texture
    pipeline (`Event::TextureNotFound`,
    `Command::RequestTexture`/`FetchTexture`, `Session::request_texture` + the
    `send_request_image` codec, both runtimes' HTTP `GetTexture` fetch fns);
    and the sl-wire `LegacyMaterial.normal_map`+`specular_map` (the
    `RenderMaterials` capability's explicit per-map "texture id"s).
    `ProfileUpdate` lost its `Default` derive (`TextureKey` is not `Default`)
    → equivalent manual impl. **Left raw (deliberately):**
    `SculptData.texture` (a mesh-*or*-texture union discriminated by
    `sculpt_type`'s `MESH` bit — typing it `TextureKey` would be *wrong* when
    it holds a mesh asset; deferred to the union-key item), the GLTF/legacy
    *material* asset ids
    (`RenderMaterialRef`/`TextureFace`/`RenderMaterialEntry` `material_id`,
    `MaterialOverrideUpdate.asset_id` — a material is not a texture), every
    generic `asset_id` and `Asset.id` (variable asset class), and the
    `RegionHandshake` `terrain_detail0..3` (only nil placeholders in the
    generated message blocks, no hand-written typed surface). The
    `texture_downloads` map stays keyed by `Uuid` (`TextureKey` has no `Ord`).
    Re-exported `TextureKey` through
    `sl-proto`/`sl-client-tokio`/`sl-client-bevy` (parity); REPL parses the
    raw `Uuid` then wraps, examples wrap at the texture-id definition;
    `sl-survey` unaffected (no texture handling). Also folded in a
    user-requested AgentKey-sweep fix: `AvatarAppearance.avatar_id` `Uuid` →
    `AgentKey`. +1 focused unit test (`types::asset`
    `texture_key_round_trips_raw_uuid`: wrap/unwrap is the identity, incl. the
    nil default); lifecycle + `sim_session` round-trip suites updated. NO
    sl-types touched.
