---
id: viewer-spacenav-ignore-when-window-unfocused
title: SpaceNavigator drives the viewer even when its window is not focused
topic: viewer
status: done
origin: noticed while profiling shadow cost on aditi (2026-08-01)
refs:
  [viewer-input-spacenav-device]
---

Context: [context/viewer.md](../context/viewer.md).

The 6-DOF SpaceNavigator keeps driving the viewer's camera / avatar motion
while the viewer window does **not** have keyboard focus — moving the device
to work in another application still pushes the flycam and avatar around in the
background.

## Cause

The Linux read half (`spacenav.rs`, `device::poll_device`, an `Update` system)
reads the device's axes directly off **evdev** (`/dev/input/event*`) and writes
them into the `SpacenavInput` resource every frame. That is a global physical
device read — it does not go through the windowing / input focus system the way
keyboard and mouse do (Bevy only delivers those to the focused window), so the
poll fills `SpacenavInput` regardless of whether the viewer window is focused,
and the flycam / `avatar_nav_drive` consumers act on it.

## Fix direction

Gate the SpaceNavigator on the primary window's focus:

- Read `Window::focused` (the `PrimaryWindow`). When the window is not focused,
  **drain and discard** the pending evdev events (so a burst does not replay on
  refocus) and zero `SpacenavInput` (so no residual axis keeps driving motion).
- Only publish live axes into `SpacenavInput` while focused.

Do it at the `poll_device` read (drop the events at the source) rather than at
each consumer, so every current and future consumer of `SpacenavInput` inherits
the focus gate. Matches the reference viewer, which ignores joystick input when
the window is not in the foreground
(`indra/newview/llviewerjoystick.cpp` — the `mJoystickEnabled` /
foreground checks).

Self-centring axes report 0 at rest, so once focus is lost the zeroed input is
stable; the only care is draining buffered evdev events so regaining focus does
not apply a stale accumulated delta.

## Done

Fixed in `spacenav.rs` `device::poll_device`: it now takes
`Query<&Window, With<PrimaryWindow>>` and, when the primary window is not
focused, drains the evdev backlog, zeroes the device's cached raw axes / button
state, and publishes a zeroed `SpacenavInput` before the normal read — so every
`SpacenavInput` consumer inherits the gate. Linux + `spacenav`-feature only,
which is the sole path that reads the device (elsewhere `SpacenavInput` stays
default zero, so there is nothing to gate). Compiles clean; the "device ignored
while the viewer is backgrounded" behaviour is to be confirmed on the next live
run (needs the physical SpaceNavigator).
