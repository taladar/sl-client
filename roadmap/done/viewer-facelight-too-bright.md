---
id: viewer-facelight-too-bright
title: Facelights render far brighter than in Firestorm
topic: viewer
status: done
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

Worn **facelights** (the small attachment point-lights avatars wear to light
their face) render **much brighter** in our viewer than in Firestorm — blowing
out the face rather than the subtle fill Firestorm shows.

Likely a point-light intensity / attenuation / units mismatch: SL light
parameters (`LightData`: intensity, radius, falloff) are being converted to
Bevy `PointLight` (lumens / range / decay) with the wrong scale, so a
low-intensity worn light reads as a floodlight. Compare our SL-light →
`PointLight` conversion against how Firestorm applies avatar-attached point
lights (intensity curve, radius→range, the deferred light contribution), and
match the falloff so a facelight is a gentle fill.

## Done (2026-08-06)

Reproduced offline with the replay harness (bundle agent `52ed4c6a` carries a
worn white facelight: intensity `153/255`, radius `1.0 m`, falloff `0.75`) —
`--replay` renders it and drives the live local-light path. Two distinct
defects, both fixed in `sl-client-bevy-viewer`:

**1. Brightness (the reported bug), `lights.rs`.** The SL → Bevy conversion used
a flat `LOCAL_LIGHT_LUMENS = 1_000_000` (Bevy's `VERY_LARGE_CINEMA_LIGHT`) ×
intensity, ignored the SL `falloff` entirely, and set `range = radius` instead
of Firestorm's `radius × 1.5`. For the facelight that is ≈ 600 k lumens ≈
190 000 lux at the ~0.5 m face — ~19× the 10 000-lux scene sun. Fix ports
Firestorm's deferred light model: `calcLegacyDistanceAttenuation`
(`deferredUtil.glsl`) — a **bounded** clamped-quadratic that hits zero at the
reach — with `size = radius × 1.5` and shader `falloff = wire_falloff × 0.5`
(`DEFERRED_LIGHT_FALLOFF`). `local_light_lumens()` calibrates each light's Bevy
lumens by matching its illuminance at half its reach to the SL surface
contribution relative to `SCENE_LIGHT_ILLUMINANCE`; radius²- and falloff-aware.
The facelight now emits ≈ 11 k lumens (~53× dimmer) — a gentle fill matching
Firestorm. `range` = `radius × 1.5` for point and spot lights. The inherent
limitation is documented: Bevy point lights are pure inverse-square, so they
cannot reproduce SL's bounded near-field exactly; calibrating at mid-reach keeps
the near-field overshoot small. Unit-tested (attenuation reference curve,
facelight-is-a-fill, radius/intensity scaling).

**2. Near-camera light dropout, `lib.rs` (camera `ClusterConfig`).** With the
brightness fixed, a *pre-existing* Bevy clustered-forward artifact became
visible: the facelight dropped out of a mid camera-distance band (lit < ~2 m,
dark ~2–5 m, lit again past 5 m). Root cause is Bevy's default `ClusterZConfig`
— a **special first Z-slice** `[near_plane, first_slice_depth = 5 m]` whose
light handling fails for near-camera lights, plus `MaxClusterableObjectRange`
far mode letting a lone small light collapse the grid's depth range (the 5 m
re-light edge is `first_slice_depth`; the ordinary log slices past it light
correctly). `ClusterConfig::Single` made it worse (dark everywhere but the
inside-sphere case), confirming the Z-config as the cause. Fix sets an explicit
`ClusterConfig::FixedZ` with `first_slice_depth = 0.5 m` (so an avatar viewed at
≥ 1 m always falls in the good log slices) and `far_z_mode = Constant(512 m)`
(so a lone small light can't collapse the range). Verified live: no lighting
difference at any camera distance. This helps **every** small local light, not
just facelights.

Two unrelated issues on the same avatar were split out for their own sessions:
[viewer-attachment-earrings-not-rigged](../bugs/viewer-attachment-earrings-not-rigged.md)
and
[mesh-hair-and-hairbase](../bugs/viewer-avatar-mesh-hair-and-hairbase-both-render.md).
