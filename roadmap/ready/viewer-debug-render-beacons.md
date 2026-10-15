---
id: viewer-debug-render-beacons
title: Debug render beacons (physics / scripted / sound / particle markers)
topic: viewer
status: ready
origin: user request (2026-07)
refs: [viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

The *other* beacons — the debug markers the reference viewer's beacons floater
toggles, which the user-facing tracking beacon ([[viewer-beacons-beam-render]])
explicitly excludes. These are diagnostic: a coloured vertical marker (and
optionally a highlight of the object itself) over every object matching a class
— physical, scripted, touch-enabled, sound-source, particle-source, media — plus
the "render highlights" and "hide particles" companions.

Everything they mark is already in the ECS scene mirror: the object flags (the
`Object` physics / script / touch bits), `Object.sound` for a sound source, the
P30 particle systems for a particle source, and the media entries for media
prims. So this is the marker rendering plus the per-class toggles.

Render the markers **gizmo-based** (cheap `bevy_gizmos` lines rather than
spawned mesh entities) from the scene mirror, and drive the per-class toggles
from an env var / console toggle rather than a floater — it is a debugging
feature first, so the toggles should be reachable without a mouse round-trip
while live-testing (a keybind via [[viewer-input-action-map]] is the eventual
home).

Audit 2026-10-15: a marker renderer now exists —
`sl-viewer-world-scene/src/debug_beacons.rs` (`DebugBeaconPlugin`) draws the
reference's `addDebugBeacon` crosses (see-through + solid pass, thin boxes
since `wgpu` has no line width, anchored to a live object) for whatever an
open floater puts in the shared `DebugBeacons` resource
(`sl-viewer-social/src/map_tracking.rs`); the telehub floater is its first
caller. Per-class beacons should feed that resource (or share its material)
rather than add a second marker path; the gizmo suggestion above predates it.
What remains is the per-class sweep of the scene mirror, the toggles, and
the highlight / hide-particles companions.

Reference (Firestorm, read-only): `llfloaterbeacons`,
`LLPipeline::renderDebugBeacons` and the `sBeacon*` flags.

Builds on: the existing object flags, `Object.sound`, and the P30 particle
system.
