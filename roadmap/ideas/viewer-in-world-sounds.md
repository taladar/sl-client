---
id: viewer-in-world-sounds
title: In-world spatial sounds
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-audio-backend]
---

Context: [context/viewer.md](../context/viewer.md).

3-D positional sound from the world: `llTriggerSound` one-shots, looped and
attached object sounds, collision sounds — the layer that makes a region feel
inhabited rather than silent.

The **receive protocol is done** (`protocol-22`) and entirely unconsumed by the
viewer: `Event::SoundTrigger` (sound, owner, object, parent, region handle,
position, gain), `Event::AttachedSound` with `SoundFlags` (LOOP / SYNC_MASTER /
SYNC_SLAVE / SYNC_PENDING / QUEUE / STOP), `Event::AttachedSoundGainChange`, and
`Event::PreloadSound` — plus the per-object `sound` / `gain` / `sound_flags` /
`sound_radius` fields that already ride along on every `Object`. The send side
exists too (`Command::TriggerSound`), so gestures and scripted client sounds
have a path out.

Scope on top of [[viewer-audio-backend]]: spatialise each source against the
listener, distance attenuation and rolloff (match the reference's curve — SL
content is authored against it), attached sounds that follow their object as it
moves and stop when it is removed, looped sounds with the sync master / slave
and queue semantics the flags describe, `PreloadSound` prefetch so a triggered
clip is not late, and collision sounds (the P31 `avian3d` contacts are already
there to hang them on).

Two policies worth deciding early: **parcel-local sound** — the `SOUND_LOCAL`
bit in the parcel overlay grid that [[viewer-parcel-overlay-decode]] decodes,
which
clamps a parcel's audio to its boundary — and the source budget (SL scenes can
easily ask for more simultaneous sounds than any device wants; the reference
caps and evicts by priority and distance). Muting (per-object, per-owner, the
mute list) belongs here too.

## Beyond the reference: occlusion

Distance attenuation is the floor, not the ceiling. **Steam Audio is Apache-2.0
with full source** and the `audionimbus` binding already supports Bevy 0.19,
giving HRTF plus **occlusion and transmission against real geometry** — sound
muffled by the prim wall between you and the emitter, which no SL viewer does.
The reason it is tractable here: `audionimbus` exposes a **`CustomRayTracer`**,
so it can query the **avian3d/parry BVH we already maintain** for the prim world
rather than building and re-committing its own acoustic scene every time a prim
moves — which is exactly what would otherwise kill the idea in SL, where
geometry is rezzed and animated constantly.

Treat it as **phase 2 behind a toggle**: phase 1 is pan + distance + a low-pass,
phase 2 adds occlusion/transmission and binaural HRTF for the nearest few
emitters, with reflections/reverb last (most expensive). Open questions worth
knowing before committing: nobody has benchmarked ray queries at SL prim
densities, and **SL carries no acoustic material metadata**, so the material
model has to be synthesised (from the prim material enum, or just a default) —
a design decision, not a lookup.

Reference (Firestorm, read-only): `llaudio/llaudioengine_*`, `lldeferredsounds`,
`LLViewerObject::setAttachedSound`.

Builds on: `protocol-22` sound-receive, `sl-asset` fetch, and the P31 physics
contacts.

Deps: [[viewer-audio-backend]] (device, decode, listener, mixer).
