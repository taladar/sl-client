---
id: idiomatic-p7-05
title: Remaining nil-key fields** (apply the same nil ⇄ None boundary pattern
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 7 — second-pass audit (missed ids, in-band sentinels, non-masking)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

**Remaining nil-key fields** (apply the same nil ⇄ `None`
    boundary pattern; wire byte-identical): parcel
    `media_id`/`snapshot_id`/`auth_buyer_id`
    (`ParcelInfo`/`ParcelUpdate`/`ParcelMediaUpdateInfo`/`ParcelDetails` —
    note `snapshot_id`/`media_id` are shared with `PickInfo`/`ClassifiedInfo`/
    `DirLandResult`/`PlacesResult` and read by `sl-survey`, so convert all
    carriers together); `ObjectProperties` `folder_id`/`from_task_id`;
    `TextureFace.material_id`; `Wearable.asset_id`; group
    `ActiveGroup .active_group_id`/`GroupRole.role_id`/
    `GroupProfile.insignia_id`;
    map `TelehubInfo.object_id`/`MapItem.id`; `ScriptDialog.owner_id` (raw
    `Uuid`); `ChatSource::Object.owner_id`/`InstantMessage.region_id` (raw
    `Uuid`); `InventoryFolder.parent_id` (nil = root); editing
    `RezObjectRequest.group_id`/ `ray_target_id`. (`MuteEntry.id` is
    genuinely-keyed-or-name — see exceptions.) **DONE** (2026-06-24). Added
    two reusable codec helpers in `sl-proto/src/types.rs`
    (`optional_key_from_wire`/`optional_key_to_wire` for the typed `*Key`s,
    `optional_uuid_from_wire`/`optional_uuid_to_wire` for the deliberately-raw
    `Uuid` fields) so every site decodes nil → `None` / encodes `None` → nil
    with byte-identical wire output, plus REPL `opt_texture`/`opt_group` arg
    helpers (mirroring the existing `opt_agent`/`opt_object`). **Resolved
    decisions:** `auth_buyer_id` became `Option<AgentKey>` (not raw `Uuid`) at
    the user's request — it is the single authorised-buyer avatar. The
    `Update` carriers were converted alongside their `Info` siblings
    (`PickUpdate`/`ClassifiedUpdate`/`ParcelUpdate` snapshot/media), since the
    roadmap says "convert all carriers together". `DirLandResult` turned out
    to carry **no** `snapshot_id` (only `PlacesResult` does). The roadmap's
    `RezObjectRequest` names no real struct: interpreted as the rez-object
    request path — `Command::RezObject.group_id` plus `NotecardRez`
    (`RezObjectFromNotecard`) `group_id`/`ray_target_id`; `rez_object`'s
    public param widened `GroupKey` → `Option<GroupKey>` to match. **Follow-up
    (user-requested): also converted every *other* nil-sentinel id that was
    left raw only because it wasn't named** — `NotecardRez.from_task_id`,
    `ParcelUpdate.group_id`, the `Command`/`ServerEvent`
    `DuplicateObjects`/`DuplicateObjectsOnRay`/`DerezObjects`/`BuyParcel`
    `group_id` (+ `DuplicateObjectsOnRay.ray_target_id`; the public
    `duplicate_objects`/`duplicate_objects_on_ray`/`derez_objects`/
    `buy_parcel` method + circuit-sender params widened to `Option`),
    `GroupRoleMember`/`GroupTitle`/`GroupRoleEdit`/`GroupRoleMemberChange`
    `role_id` (nil = "Everyone"), and `CreateGroupParams.insignia_id`.
    **Deliberately NOT converted:** the *required* group ids where nil is
    meaningless (`DeedParcelToGroup`/`RequestGroupExperiences`/
    `ActivateGroup`/group-notice/profile/… — already typed `GroupKey`), and
    `SkeletonFolder.parent_id` (purely sl-wire — mapped to `Option` at the
    `skeleton_folder` conversion boundary instead). +2 focused unit tests
    (`optional_key`/`optional_uuid` nil↔None round-trip); lifecycle +
    `sim_session` suites updated; book `content/region.md` (telehub id). NO
    sl-types touched (all client wire concepts / consumed keys). Build +
    clippy (--workspace --all-targets) + 718 tests green.
