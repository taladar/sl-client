---
id: server-lsl-lib-ossl-avatars
title: Library tranche — OpenSim's NPC and avatar functions
topic: server
status: done
origin: split from server-lsl-lib-avatar-control (2026-11-07)
blocked_by: [server-lsl-lib-avatar-control]
refs: [server-lsl-lib-ossl, server-fake-grid-scripted-avatars]
---

Context: [context/lsl.md](../context/lsl.md).

`server-lsl-lib-avatar-control` implemented the `ll*` avatar functions — the
queries, animations and overrides, controls and camera — over the host's
`vm::avatars` records and messages. OpenSim's 66 functions that shadow them are
this task's, listed `later` under this id in
`sl-lsl-runtime/src/library/opensim-scope.txt`; they are compiled by OpenSim's
dialect and gated by `vm::ossl::admit` already:

- **NPCs** (`osNpc*`, `osIsNpc`, `osGetNPCList`, `osNpcGetOwner`, …): the fake
  grid's NPC fixtures (`fixtures/npcs.rs`) as the host's records, created,
  moved, sat, animated and made to speak — the largest part, and the one most
  of OpenSim's content scripts use.
- **The avatar-side twins** of this tranche's `ll*` functions: animation
  (`osAvatarPlayAnimation`, `osAvatarStopAnimation`), attachments
  (`osForceAttachToAvatar*`, `osForceDetachFromAvatar`, `osDropAttachment*`,
  `osGetNumberOfAttachments`, `osMessageAttachments`), sitting and stand
  targets (`osForceOtherSit`, `osSet/GetStandTarget`, the sit active range),
  teleports (`osTeleportAgent`, `osTeleportOwner`, `osLocalTeleportAgent`),
  names (`osKey2Name`, `osAvatarName2Key`, `osAvatarType`, `osGetGender`),
  appearance saves, health and speed, group invitation and ejection, and
  `osKickAvatar`.

Port each from `OSSL_Api.cs`, register it in `library/registry.rs`, move its
scope line to `in`, and drop the written-in answers of `ossl/available` and
`ossl/errors` (`osGetGender`, `osIsNpc`, …) in `vm/tests/ossl.rs`.

## From server-lsl-forced-delays-per-flavour (2026-11-07)

`osNpcSay`, `osNpcSayTo` and `osNpcShout` call `LSL_Api.ThrottleSay(channel,
2000)`: the script's channel-0 chat throttle, `vm::delays::SayThrottle`, which
`llSay` / `llShout` go through (`library/comms.rs`, `waits_for_throttle`).
`osTeleportAgent` sleeps 0.5 s on a refusal and 5 s on a teleport, and
`osMessageAttachments` a penalty of its own; say them with `ScriptCtx::delay`.

## Done (2026-11-14)

All 66 functions (78 overloads) are in `library/ossl_avatars.rs`, their scope
lines `in`. `vm/npcs.rs` holds what the host lends and is asked: `Npc`,
`NpcSpec`, `NpcAction`, `NpcSpeech`, `Vitals`, `DropAt`, and `Appearance` with
OpenSim's notecard format (`PackForNotecard`'s key order and indentation,
Mono's `ToString("g")` of a double, read back over the default avatar as
`Unpack` reads it; `DEFAULT_VISUAL_PARAMS` moved here from the fake grid's NPC
fixtures). The `Host` grew `create_npc`, `npcs`, `npc_act`, `npc_say`,
`appearance`, `vitals` / `set_vitals`, `alert`, `set_speed`, `kick`,
`group_powers` / `group_name` / `invite_to_group` / `eject_from_group`,
`attachments`, `attach_from_inventory`, `drop_attachment` and `sit`;
`make_notecard` replaces by name and answers the asset; `Seat` keeps a stand
target and a sit range; `Destination` has `Region` and `Moved`; `Sensed` has
`npc` and the sweep admits and types an NPC as OpenSim's `SensorRepeat` does;
`Exception` has `InvalidUuid` (`new UUID(key)`) and the two item-named throws
of a wear from the contents (so it is no longer `Copy`). `osTeleportAgent`'s
overload taking no region is ungated, as in OpenSim: `ossl-checks.txt` takes
a `name#N` line for one overload and `build.rs` reads it. The `ossl/available`
replay reads gender, health and `osIsNpc` from the host now.

The fake grid shows an NPC as a resident with no session (`scripts/npcs.rs`):
it walks, runs or flies at the grid's speeds each tick, sits and stands,
speaks as an avatar, touches as a click (`touches::click`, the touch path
taking a position instead of a session), looks at things (the `LookAt`
effect), is answered as an avatar by the `ll*` readers and the sensor, and is
removed by `osNpcRemove`, a kick or death. Every avatar's health is kept and
healed each tick, its viewer sent `HealthMessage`; a death alerts and sends
home. `osSetSpeed` scales the session's movement policy
(`MovementPolicy::scaled`); a kick ends every session of the agent
(`login_endpoint::kick`); a group invitation or ejection goes in the owner's
name with no session (`groups::invite_from_object`, `eject_from_object`); a
wear from the contents files a copy and wears it in the wearer's session; a
drop shares the viewer's Drop (`world::drop_worn`); a forced sit takes the
next free sit target within the prim's range; a stand target puts a stand-up
where OpenSim puts it; a teleport by region name or a local move without a
teleport sequence go through `teleport::scripted` — and the client now
surfaces a `TeleportLocal` that comes with no teleport started, as the
reference viewer applies it in any teleport state. Runtime tests in
`vm/tests/ossl_avatars.rs`; fake-grid tests
`an_npc_appears_speaks_walks_sits_touches_and_leaves` and
`ossl_teleports_move_the_owner_or_name_the_missing_region`.

## Left open

- **An NPC's profile** (`osNpcSetProfileAbout`, `osNpcSetProfileImage`): the
  fake grid keeps no profiles, so both log that they are not modelled and do
  nothing; noted on [[server-fake-grid-profiles]].
