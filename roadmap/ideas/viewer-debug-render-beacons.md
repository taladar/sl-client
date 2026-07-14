---
id: viewer-debug-render-beacons
title: Debug render beacons (physics / scripted / sound / particle markers)
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

The *other* beacons — the debug markers the reference viewer's beacons floater
toggles, which [[viewer-beacons]] explicitly excludes (that task is the
user-facing **tracking** beam). These are diagnostic: a coloured vertical marker
(and optionally a highlight of the object itself) over every object matching a
class — physical, scripted, touch-enabled, sound-source, particle-source, media
— plus the "render highlights" and "hide particles" companions.

Everything they mark is already in the ECS scene mirror: the object flags (the
`Object` physics / script / touch bits), `Object.sound` for a sound source, the
P30 particle systems for a particle source, and the media entries for media
prims. So this is the marker rendering plus the floater's per-class toggles and
their persisted state.

It is a debugging feature first: it should be usable to answer "why is this
object heavy / noisy / scripted" while live-testing, so keep the markers cheap
and make the toggles reachable without a mouse round-trip (a keybind, per
[[viewer-input-system]]).

Reference (Firestorm, read-only): `llfloaterbeacons`,
`LLPipeline::renderDebugBeacons` and the `sBeacon*` flags.

Builds on: the existing object flags, `Object.sound`, and the P30 particle
system.

Deps: [[viewer-ui-framework]] (the floater and its toggles).
