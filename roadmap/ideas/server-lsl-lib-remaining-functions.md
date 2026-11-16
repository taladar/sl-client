---
id: server-lsl-lib-remaining-functions
title: Library — the 44 ll functions no tranche took
topic: server
status: ideas
origin: server-lsl-lib-physics-vehicles (2026-11-15)
refs: [server-lsl-library-surface-table, server-lsl-library-table-refresh,
  server-lsl-lib-email-xmlrpc]
---

Context: [context/lsl.md](../context/lsl.md).

With the physics tranche done, every library tranche the audit planned is
done or deferred, and the coverage test still counts 52 `ll*` functions
missing of 524. Eight are email and XML-RPC
([[server-lsl-lib-email-xmlrpc]], deferred). The other 44 belong to no task:
most came with the 2026-09-27 table refresh, after the tranches were cut, and
a few were left by the tranche whose subject they share. A script calling one
stops with `<name> is not implemented by this grid`.

By what they touch:

- **Damage and combat**: `llDamage`, `llAdjustDamage`, `llDetectedDamage`
  (with the stubbed `llSetDamage` and `llGetHealth`).
- **Detection**: `llDetectedRezzer`.
- **Notecards**: `llFindNotecardTextCount`, `llFindNotecardTextSync`.
- **Avatars and agents**: `llGetAttachedListFiltered`, `llGetVisualParams`,
  `llIsFriend`, `llSameGroup`, `llSetAgentRot`, `llGiveAgentInventory`,
  `llRequestInventoryData`, `llMapBeacon`, `llOpenFloater`,
  `llWorldPosToHUD`.
- **Camera**: `llGetCameraAspect`, `llGetCameraFOV`, `llReleaseCamera`,
  `llTakeCamera`, `llSetLinkCamera`.
- **Objects**: `llGetObjectLinkKey`, `llGetObjectPermMask`,
  `llSetObjectPermMask`, `llTransferOwnership`, `llReturnObjectsByID`,
  `llReturnObjectsByOwner`, `llGodLikeRezObject`.
- **Land and environment**: `llModifyLand`, `llSetGroundTexture`,
  `llSetParcelForSale`, `llSetEnvironment`, `llReplaceEnvironment`,
  `llSetAgentEnvironment`, `llReplaceAgentEnvironment` (the fake grid's
  environments, `server-lsl-lib-ossl-land-writers`, are there to write).
- **Events**: `llMinEventDelay`.
- **Cryptography**: `llSignRSA`, `llVerifyRSA`.
- **Deprecated**: `llMakeExplosion`, `llMakeFire`, `llMakeFountain`,
  `llMakeSmoke` (particle systems, as `llParticleSystem`), `llSetPrimURL`,
  `llRefreshPrimURL`.

Split it into tranches by subject when it is picked up, each measured by a
probe first as the earlier tranches were (the `gridspec-lsl-*` discovery
tasks), and take OpenSim's own shadows of each with it.
