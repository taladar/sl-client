---
id: viewer-facelight-too-bright
title: Facelights render far brighter than in Firestorm
topic: viewer
status: bugs
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
