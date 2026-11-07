---
id: server-lsl-lib-ossl-avatars
title: Library tranche — OpenSim's NPC and avatar functions
topic: server
status: ready
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
