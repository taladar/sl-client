---
id: server-lsl-lib-prim-state
title: Library tranche — reading and writing the prim
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-ecs-store,
  server-world-link-sets, server-world-update-scheduling]
refs: [server-lsl-lib-detection-sensors, viewer-fake-grid-render-catalogue]
---

Context: [context/lsl.md](../context/lsl.md).

The biggest world-touching tranche and the one the viewer can *see*, so
it is the one worth doing first after the plumbing: every function here
ends in an object update, and a wrong one is visible in a frame.

~90 functions in four groups:

- **Placement** — `llGetPos`, `llSetPos`, `llSetRegionPos`,
  `llGetLocalPos`, `llGetRootPosition`, `llGetRot`, `llSetRot`,
  `llGetLocalRot`, `llGetRootRotation`, `llGetScale`, `llSetScale`,
  `llGetBoundingBox`, `llGetGeometricCenter`, `llTargetOmega`. Note that
  `llSetPos` moves at most 10 m per call on a non-physical prim and
  *silently clamps* — content relies on the clamp. `llTargetOmega` is
  **client-side**: it is an angular velocity on the object update the
  viewer integrates, not a server-side rotation, so the "cheap spin"
  costs the region nothing and the region never learns the current
  angle.
- **Appearance** — `llSetColor`, `llSetAlpha`, `llSetTexture`,
  `llSetTextureAnim`, `llScaleTexture`, `llOffsetTexture`,
  `llRotateTexture`, `llGetColor`/`Alpha`/`Texture`, `llSetLinkColor`
  and the rest of the `llSetLink*` family, `llSetText`,
  `llSetLinkPrimitiveParams`, `llSetLinkPrimitiveParamsFast`,
  `llGetPrimitiveParams`, `llGetLinkPrimitiveParams`, `llSetPrimMediaParams`,
  `llParticleSystem`, `llLinkParticleSystem`. The `PRIM_*` parameter
  list is itself a sub-language — a flat list of parameter codes and
  their arguments — and is best implemented once as a codec over the
  `Object` components rather than function by function.
- **Identity and inventory-adjacent** — `llGetKey`, `llGetObjectName`,
  `llSetObjectName`, `llGetObjectDesc`, `llSetObjectDesc`,
  `llGetObjectDetails` (a second parameter-code sub-language, over *any*
  key in the region), `llGetOwner`, `llGetCreator`, `llGetLinkNumber`,
  `llGetNumberOfPrims`, `llGetNumberOfSides`, `llSetClickAction`,
  `llSetPayPrice`.
- **Sound and media** — `llPlaySound`, `llLoopSound`,
  `llLoopSoundMaster`/`Slave`, `llTriggerSound`,
  `llTriggerSoundLimited`, `llStopSound`, `llPreloadSound`,
  `llSetSoundQueueing`, `llSetSoundRadius`, `llAdjustSoundVolume`.
  `SimSession` already has `send_sound_trigger`, `send_attached_sound`,
  `send_attached_sound_gain_change` and `send_preload_sound`, so this
  group is mostly wiring.

The `Object` model in `sl-proto` already carries every field these
touch — `text`/`text_color`, `particle_system`/`particles`,
`texture_anim`/`texture_animation`, `sound`/`gain`/`sound_flags`/
`sound_radius`, `extra_params`/`extra` — with encoders
(`encode_particle_system`, `encode_texture_anim`) already written. The
work is the parameter codec and the change plumbing, not the wire.

Acceptance: a scripted prim in the fake grid's scenario changes colour,
texture, text, scale and particles on a timer, and both viewers render
the change (`sl-crosscheck` frames agree); `llGetPrimitiveParams` round-
trips everything `llSetPrimitiveParams` can set; and `llSetPos` clamps
at 10 m.
