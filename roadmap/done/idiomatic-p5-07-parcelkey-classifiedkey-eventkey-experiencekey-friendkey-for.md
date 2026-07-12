---
id: idiomatic-p5-07
title: ParcelKey / ClassifiedKey / EventKey / ExperienceKey / FriendKey for t
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 5 — Typed UUID keys from `sl-types` (most invasive, top value)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`ParcelKey` / `ClassifiedKey` / `EventKey` / `ExperienceKey` /
    `FriendKey` for the remaining role-specific id fields. Replaced every raw
    role-specific id with the matching typed newtype, wrapping at the codec
    boundary only (decode `Key::from`/`EventId::new`, encode `.uuid()`/
    `.get()`; builders stay wire-identical via `Display`) so wire bytes are
    byte-identical. NO sl-types change — all five `sl-types` keys already
    carry `Copy`/`Hash`/`uuid()`/`From<Uuid>`/`Display` from the AgentKey
    sweep's 0.4.0.
    **`EventKey` was wire-inapplicable → a new repo-local `EventId(pub u32)`
    newtype instead (user-directed):** SL events-directory ids are a numeric
    `u32` (`DirEventResult`/`EventInfo` `event_id`, the three `Event*Request`
    commands), not a UUID, so the `Key(Uuid)`-shaped `EventKey` fits nothing.
    Added a public `EventId` (`new`/`get`/`Display`, modelled on
    `RegionLocalObjectId`) **in `sl-proto`, not `sl-types`**, typing every
    `event_id` across the client `Command`s, `EventInfo`/`DirEventResult`, the
    `Session` event methods + circuit senders, and the server-side
    `ServerEvent::Event*Request` + `SimSession` decode/encode.
    **ParcelKey:** `parcel_id` on `PickInfo`/`ClassifiedInfo`/`PickUpdate`/
    `ClassifiedUpdate`/`DirPlaceResult`/`DirLandResult`/`ParcelDetails`,
    `Event::ParcelDwell`/`RemoteParcelId`, `Command::RequestParcelInfo`/
    `RequestLandResources`, `ServerEvent::RequestParcelInfo`, the
    `request_parcel_info` method + circuit sender, and the sl-wire
    remote-parcel / land-resources codec helpers. **ClassifiedKey:**
    `classified_id` on `AvatarClassified`/`ClassifiedInfo`/`ClassifiedUpdate`/
    `DirClassifiedResult`, the `Command::RequestClassifiedInfo`/
    `DeleteClassified`/`GodDeleteClassified` trio, and the three Session +
    circuit-sender pairs.
    **ExperienceKey:** `ExperienceInfo.public_id`,
    `ExperienceUpdate.public_id` (lost its `Default` derive → manual impl),
    the experience `Event`s + `Command`s, and the full sl-wire experience
    cap codec (client + server helpers); `group_experiences_query(group_id)`
    stays raw (a group id). **FriendKey:** `Friend.id`,
    `Event::FriendsOnline`/`FriendsOffline`/`FriendRightsChanged.friend_id`,
    `Command::GrantUserRights.target`/`TerminateFriendship`, the two Session +
    circuit-sender pairs.
    **Nil-sentinel fields on user-exposed structs became `Option` (and an
    agent-or-group owner an `OwnerKey`), per a user-stated rule:**
    `ExperienceInfo`'s `agent_id`+`group_id` collapsed to
    `owner: Option<OwnerKey>` (`None` = placeholder; the codec splits it back
    to the two wire fields); `ScriptPermissionRequest.experience_id` →
    `Option<ExperienceKey>`; `AvatarProperties.partner_id` →
    `Option<AgentKey>`; `PickUpdate`/`ClassifiedUpdate.parcel_id` →
    `Option<ParcelKey>` (`None` = use the agent's current parcel). Also fixed
    the GroupKey-sweep miss `ExperienceInfo.group_id`.
    Re-exported the keys + `EventId` through `sl-proto`/`sl-client-tokio`/
    `sl-client-bevy` (parity); REPL / runtimes / examples updated. +3 focused
    unit tests; lifecycle + `sim_session` + sl-wire round-trip suites updated
    (691 tests green). NO sl-types touched.
