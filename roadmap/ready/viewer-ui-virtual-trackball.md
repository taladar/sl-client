---
id: viewer-ui-virtual-trackball
title: Virtual trackball widget (sun / moon direction)
topic: viewer
status: ready
origin: viewer-environment-personal-lighting (2026-09-09)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-environment-personal-lighting, viewer-environment-fixed-editor, viewer-phototools]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's `LLVirtualTrackball`: a round control showing a direction on a
hemisphere — drag the marker to aim the sun or the moon, with the north / east
ring drawn round it and a "sun/moon is below the horizon" state. Every
environment editor has two of them.

Scope: the widget (drag to aim, keyboard nudge, the disabled state), its
[[viewer-ui-interaction-contracts]] entry, and adopting it in the environment
editors alongside the azimuth / elevation sliders those ship with today.

## Why it is its own task

[[viewer-environment-personal-lighting]] shipped the sun and moon as **two
sliders each** — azimuth and elevation, in degrees. That is the whole of the
state: `SkySettings::sun_rotation` is one quaternion, and
`sl_proto::azimuth_altitude_to_rotation` /
`sl_proto::rotation_to_azimuth_altitude` already convert both ways with a
round-trip test. So the trackball is a **second way to drive fields that are
already driven**, not a missing capability — nothing is unreachable without it.

What it buys is aiming *by pointing*: picking a sun position off a hemisphere is
a different (and for photography, better) act than typing two angles, which is
why the reference offers both side by side and why
[[viewer-phototools]] will want it too.

Reference (Firestorm, read-only): `llvirtualtrackball.cpp` / `.h`,
`widgets/virtual_trackball.xml`, and its two uses in
`floater_adjust_environment.xml` (`sun_rotation`, `moon_rotation`) — note the
reference keeps the azimuth / elevation spinners beside each trackball and
writes each from the other, which is the arrangement to match rather than
replace.

Builds on: the angle conversions and the `SkyKnob::SunAzimuth` /
`SunElevation` / `MoonAzimuth` / `MoonElevation` knobs in
`sl-viewer-environment::personal_lighting`, which the widget writes through
unchanged.
