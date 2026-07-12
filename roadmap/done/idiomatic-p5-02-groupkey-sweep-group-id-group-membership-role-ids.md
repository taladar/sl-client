---
id: idiomatic-p5-02
title: GroupKey sweep (group_id, group membership/role ids)
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`GroupKey` sweep (`group_id`, group membership/role ids). Replaced every
unambiguous **group** id with `sl_types::key::GroupKey` (already had
`Copy`/`Hash`/`uuid()`/`From<Uuid>` from the `AgentKey` sweep — no `sl-types`
change needed). For the **role** ids the roadmap bundled here (`role_id`,
`owner_role`) a new public **`GroupRoleKey(pub Uuid)`** newtype was added in
`sl-proto/src/types/group.rs` (user-approved, kept **client-only** in this
repo rather than `sl-types` — group-role ids never surface in the non-client
tooling `sl-types` serves; mirrors the `*Key` shape:
`From<Uuid>`/`uuid()`/`Display`, `Copy`/`Eq`/`Hash`). Keeping group vs role as
distinct types makes a role↔group mix-up a compile error. Maximal scope:
converted the `group.rs` carriers (`ActiveGroup.active_group_id`,
`GroupMembership`/`GroupProfile`/`GroupAccountSummary`/`GroupAccountDetails`/
`GroupAccountTransactions` `group_id`; `GroupRole`/`GroupRoleMember`/
`GroupTitle`/`GroupRoleEdit`/`GroupRoleMemberChange` `role_id`;
`GroupProfile.owner_role`), the `group_id` fields on `AvatarGroupMembership`/
`DirGroupResult`/`InventoryItem`/`NotecardRez`/`RestoreItem`/object/parcel
types, **every** group-bearing `Command` (incl. the tuple variants
`ActivateGroup`/`JoinGroup`/… and the `InviteToGroup` invitees now
`Vec<(AgentKey, GroupRoleKey)>`), `Event`, and `ServerEvent` variant, and the
~30 `Session`/circuit-sender/`SimSession` method params. **Left raw
(deliberately):** `RequestGroupNotice(Uuid)`/`notice_id` (a notice id),
`request_id`/`vote_id`/`candidate_id` (proposal/correlation ids),
`GroupNoticeAttachment.{item_id,owner_id}` (Inventory/Owner families),
`StartConference.invitees` (agents — `AgentKey` family). Codec wraps at the
boundary (decode `GroupKey::from`/`GroupRoleKey::from`, encode `.uuid()`) so
the wire bytes / LLSD `GroupID` fields are byte-identical. The internal
`OutgoingIm.to_agent_id` was **reverted from `AgentKey` to a plain `Uuid`**
(it is `pub(crate)`, never public): the `ImprovedInstantMessage` `ToAgentID`
field is dialog-discriminated — an agent for a 1:1 IM, a group for a group
notice / group-session message, an ad-hoc session id for a conference message
— so no single typed key fits, and the prior `AgentKey` typing was a misnomer
(`send_conference_message` did `AgentKey::from(session_id)`). Callers now
pass the raw `Uuid` for their dialog (`group_id.uuid()` for the notice,
`agent.uuid()` for real IMs, the session id verbatim for conferences); the
public method params stay correctly typed (`group_id: GroupKey`,
`session_id: Uuid`). `GroupKey`+`GroupRoleKey` re-exported through
`sl-proto`/`sl-client-tokio`/`sl-client-bevy`; the CAPS group helpers
(`fetch_group_members`/`fetch_group_experiences` + bevy mirrors) take
`GroupKey` and unwrap only at the sl-wire `build_group_member_data_request`/
`group_experiences_query` boundary; REPL parses the raw `Uuid` then wraps,
survey unchanged. +2 unit tests (`GroupRoleKey`↔`Uuid` bit-identical
round-trip incl. the nil "Everyone" role; group/role keys are distinct types);
lifecycle + `sim_session` suites updated. NO sl-types touched.
