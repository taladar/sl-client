---
id: viewer-ui-virtual-trackball
title: Virtual trackball widget (sun / moon direction)
topic: viewer
status: in-progress
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

## Done

`sl-viewer-ui-widgets::ui_trackball` is the widget, and all three environment
editors host a pair.

**The state is the two angles, not a quaternion.** The reference's control
carries an `LLQuaternion` and converts at every edge; here `TrackballAim` is the
azimuth and the elevation in degrees, which is exactly what the four `SkyKnob`s
already are — so the widget never learns what a sky is, and the `sl_proto`
conversions stay where they were.

**The projection is one cosine each way.** The disc is the hemisphere seen from
directly above, so a direction's distance from the centre *is* the cosine of its
height (`aim_to_disc` / `disc_to_aim`). What that cannot say is which side of
the horizon a body is on — a sun 30° up and one 30° down land on the same point
— so the marker carries it (filled above, hollow below), a drag keeps the
hemisphere it started in, as the reference's does, and the arrow keys are what
crosses it.

**One circle, not three.** The aiming circle, the drawn disc and the marker's
travel are all `RADIUS` — half the control less half a marker. A marker that
reached the outer edge would hang outside its own parent, which the layout
sweep's containment check calls a violation and a user sees as a dot drawn over
whatever is beside it. The slider thumb's inset argument, on a circle.

**The pair drives itself, in `rows.rs`.** `AimKnobs` is the sun's and the moon's
knob pairs as one thing; `AimTrackball` and `AimSlider` tag the controls with
the window they are in, and two systems keep each trackball and its two sliders
showing the same direction whichever the hand moved. Scoped by the window's
element prefix because three windows draw a sun trackball and any two can be
open at once over different skies.

The values travel between the widgets rather than back out of the sky, and that
is load-bearing: a rotation pointing exactly at a pole has no azimuth stored in
it, so a re-read would hand the compass back a zero nobody typed. For the same
reason `AimKnobs::write` writes the height first and the compass second.

**Adopted in all three editors** — Personal Lighting, whose sun-and-moon column
now opens each body with its trackball (the window grew 80 px to hold them), and
the fixed sky editor and the day-cycle editor through one new `TabPage` field,
`aims`, so the sun-and-moon page carries its two trackballs in the same table
that says which knobs are on it.

## Not done — and why

- **No four rotate buttons round the rim.** The reference wraps the disc in
  top / bottom / left / right buttons that roll the direction 3° about a world
  axis, and maps the arrow keys onto them — inverted, so `KEY_DOWN` calls
  `onRotateTopClick`. The arrow keys here step the two angles directly, by the
  same 3°, which is what the sliders beside the control would do.
- **No Ctrl-drag rolling mode.** The reference's `DRAG_SCROLL` accumulates
  rotations from the pointer *delta* rather than aiming at its position. Its one
  advantage over aiming is that it can leave the hemisphere, which the keyboard
  here does without a hidden modifier.

## Verified

`cargo test --release -p sl-viewer-ui-widgets` — 222 green, 17 of them the
trackball's: the projection round-trips over both hemispheres, the poles and the
horizon; the compass is a map with north up; both hemispheres project onto the
same point; a drag keeps its hemisphere; a point past the rim is refused; the
centre keeps the azimuth it was handed; a marker at the horizon stays inside the
control. Six drive the real pointer and keyboard on the laid-out control — a
drag aims where the hand is, up the screen is north, a drag off the disc holds
its last aim and still commits, a press in the square's corner is inert, the
arrow keys step the two angles and cross the horizon, and a disabled control
answers neither and hides its compass letters.

`cargo test --release -p sl-viewer-environment` — 59 green, including the pair
wiring (a slider aims its trackball and only the angle it is; a trackball drives
both its sliders; the sun leaves the moon alone; one window does not drive
another; the pair stops writing once it agrees; a trackball's angle is clamped
to its slider's range), the aim round-trip through the knob pair, the
zenith-azimuth ordering, and that every angle knob is in exactly one pair.

`cargo test --release -p sl-client-bevy-viewer -- ui_contract ui_test` — 29
green. The gallery element `sun-moon-trackball` puts the control in the whole
element sweep (every script, direction, font size, scale, and both gesture
alphabets), and its contract rows pin the reactions with **probes**, because the
control emits a `ValueChange` rather than a `UiAction` and would otherwise sweep
as inert.

The first run of that sweep corrected the table rather than the code, which is
what pinning is for: `DragAcross` was written expecting the press's own aim to
survive, and the truth is better — the drag keeps aiming for the part of its
travel that is still on the disc, and only stops when it leaves, so the control
ends up off both the pole and the horizon.

Not verified live: the two tabbed editors and the Personal Lighting window were
not driven against a grid. Nothing here depends on what a grid sends — the
control edits a sky already in hand — so what a live run adds is a person's eye
on the layout.
