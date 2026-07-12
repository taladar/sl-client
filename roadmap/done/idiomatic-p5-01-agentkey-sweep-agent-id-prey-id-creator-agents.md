---
id: idiomatic-p5-01
title: AgentKey sweep (agent_id, prey_id, creator agents, …)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`sl-types` exports `AgentKey`, `GroupKey`, `ObjectKey`, `InventoryKey`,
`InventoryFolderKey`, `TextureKey`, `ParcelKey`, `ClassifiedKey`, `EventKey`,
`ExperienceKey`, `FriendKey`, and the `OwnerKey` enum — all `Key(pub Uuid)`
wrappers, so wire conversions are mechanical. Replacing the ~196 raw
`pub …: Uuid` fields across `types/*.rs` with the correct typed key makes
"passed a group id where an agent id was expected" a compile error. Split per
type-family across several commits.

`AgentKey` sweep (`agent_id`, `prey_id`, `creator` agents, …). Replaced
every raw `Uuid` field that is unambiguously an **avatar** with
`sl_types::key::AgentKey`, wrapping at the codec boundary only (wire bytes
byte-identical). **sl-types change (user-approved via AskUserQuestion, the
only edit to the shared crate):** added `Copy, Hash` to `Key`, all the `*Key`
newtypes and `OwnerKey`, plus `pub const fn uuid(&self) -> Uuid` and
`impl From<Uuid>` on each key (`OwnerKey` got `uuid()` only). `Ord` was
deliberately *not* added
(nothing in this family keys a `BTreeMap`/set by an agent; arbitrary on a
random UUID / variant-first on `OwnerKey`; deferred to the inventory/texture
families that genuinely need it). Construction idiom `AgentKey::from(uuid)`,
extraction `key.uuid()`. Converted fields: own agent (`Circuit.agent_id`,
`SimSession.agent_id`, the `Session`/`SimSession` `agent_id()` accessors,
`LoginSuccess.agent_id`); IM/chat (`InstantMessage.{from,to}_agent_id`,
`InventoryOffer.from_agent_id`, the `to_agent_id` `Command`s, `OutgoingIm`);
presence (`CoarseLocation.agent_id`, `ViewerEffect.agent_id`,
`ViewerEffectData::{LookAt,PointAt}.source` — *not* `Spiral.source`/any
`target`, which are objects); tracking (`prey_id`); group
(`ActiveGroup.agent_id`, `GroupMember.agent_id`,
`GroupRoleMember`/`GroupRoleMemberChange.member_id`,
`GroupProfile.founder_id`, `vote_initiator`, the `member_ids:
Vec`/`&[AgentKey]`); creators (`creator_id`/`creator` on
inventory/object/editing/pick/classified/event); profile
(`AvatarProperties.{avatar_id,partner_id}`, `AvatarInterests.avatar_id`);
directory (`DirPeopleResult.agent_id`, `AvatarPickerResult.avatar_id`); the
agent-bearing `Event`/`ServerEvent` variants; `ExperienceInfo.agent_id`; the
server-side `build_map_*_reply`/`send_viewer_effect` agent params;
`compute_im_session_id`. **Left for later families (deliberately not
touched):**
`owner_id`/`last_owner_id`, money `source_id`/`dest_id` (agent-or-group →
OwnerKey), all `group_id`/`role_id` (GroupKey), object/task ids (ObjectKey),
`item_id`/`folder_id` (InventoryKey), texture/`insignia`/`snapshot` ids
(TextureKey), `parcel_id` (ParcelKey), `classified_id` (ClassifiedKey),
`Friend.id` (FriendKey), chat `source_id`. `ExperienceInfo` lost its `Default`
derive (AgentKey has no `Default`) → equivalent hand-written impl. Re-exported
`AgentKey`/`Key` through `sl-proto`, `AgentKey` through
`sl-client-tokio`/`sl-client-bevy` (parity; `Client::agent_id()` and bevy
`SlIdentity.agent_id` now `Option<AgentKey>`); REPL parses the raw `Uuid` then
wraps, survey unwraps `.uuid()` for its raw-`Uuid` records. +1 focused unit
test (AgentKey↔Uuid bit-identical round-trip; IM `from_agent_id` survives an
`InventoryOffer` extraction). Build + clippy (--workspace --all-targets) + 678
tests green.
