---
id: server-lsl-lib-prim-state
title: Library tranche — reading and writing the prim
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-ecs-store,
  server-world-link-sets, server-world-update-scheduling,
  gridspec-lsl-prim-state]
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

## Measured (gridspec-lsl-prim-state, 2026-10-27)

`book/src/gridspec/lsl.md` § Reading and writing the prim has the tables;
what the flavours must differ in:

- **Placement.** `llSetPos` caps a root at 10 m on both; a child's offset
  is capped on OpenSim only. Scale is 0.01–64 m on Second Life, 0.001–256 m
  on OpenSim. `llGetBoundingBox` follows a scale late on Second Life.
- **Values.** OpenSim stores colour, alpha and glow as bytes (0.5 reads
  0.498039); Second Life reads back what was set. Second Life refuses an
  out-of-range bump, shiny, fullbright, glow, texgen or material with a
  `bounds error` line; OpenSim clamps or converts silently. Second Life
  clamps a flexible path, a torus's hole and radius offset, and hollow to
  0.95; OpenSim keeps them. Names are 63 bytes on Second Life (127 for a
  description on both), and Second Life turns a pipe, a newline and every
  byte outside ASCII into `?`.
- **Rule lists.** An unknown code empties Second Life's whole get; OpenSim
  skips it. A value of the wrong type stops the rest of a set on Second Life;
  OpenSim converts what it can. On a single prim Second Life answers only
  link 0 and `LINK_THIS`; OpenSim also 1, `LINK_SET` and `LINK_ROOT`.
  `llGetLinkPrimitiveParams` of the `LINK_*` sets answers nothing on Second
  Life. OpenSim throws on a get of a face it cannot index.
- **Linking.** A child's `PRIM_POSITION` is ignored on Second Life and taken
  as the offset on OpenSim. The error lines differ (Second Life: `Script
  trying to link but …`); OpenSim says nothing of a bad target.
- **`changed`.** Second Life raises nothing for the texture setters and the
  material maps, `CHANGED_COLOR` for the same colour again and
  `CHANGED_RENDER_MATERIAL`; OpenSim raises `CHANGED_TEXTURE`, its own
  `CHANGED_POSITION` (32768) on a move, and two events for two kinds of
  change in one call.
- **Effects.** Second Life names a missing sound or texture and a bad media
  parameter on the debug channel and refuses an over-long media URL; OpenSim
  is silent and keeps it.
