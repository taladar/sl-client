---
id: viewer-flycam-avatar-movement-keys
title: Avatar movement keys do nothing in flycam mode
topic: viewer
status: done
origin: user report during GPU-avatar crowd testing (2026-08-14)
refs:
  - viewer-takeoff-hold-jumps-instead-of-flying
---

Context: [context/viewer.md](../context/viewer.md).

In **flycam** camera mode the avatar movement keys — arrow keys (walk / turn),
and by extension the up/down (fly) keys (PgUp / PgDn) and the other avatar
movement bindings — do nothing. The reference keeps the avatar controllable in
this mode: the flycam moves the *camera* independently, but the movement keys
still drive the *avatar*.

Confirmed again on OpenSim (2026-09-10), and the gate is now located:
`drive_avatar_controls` (`movement.rs`) **returns early** in
`CameraMode::Flycam`, after advertising a parked control set — the away bit plus
`FLY` when the avatar was already flying, so a hovering body does not plummet
when the camera detaches. Every avatar binding below that return — walk, turn,
ascend / descend, the fly toggle, the hold-to-take-off accumulator — is
therefore dead in flycam.

That early return is also why a seated avatar's `flying` flag is not tidied away
in flycam, the one edge where all three
[[viewer-takeoff-hold-jumps-instead-of-flying]] state buttons could want to show
at once. The flycam
should own only its own camera controls (the joystick / its dedicated keys) and
leave the avatar movement bindings routed to the avatar.

## Direction

Find where avatar movement input is gated by camera mode (the movement /
`AgentControl` sender and the flycam input handler). Let avatar movement keys
drive the avatar in flycam exactly as in third person / mouselook, while the
flycam keeps its independent camera motion. Watch for double-binding: any key
the flycam itself uses for camera motion must not also move the avatar (or the
two are deliberately separate key sets).

## Verify

In flycam mode on any grid: arrow keys walk / turn the avatar, PgUp / PgDn fly
up / down, and the camera still flies independently under its own controls; no
key both moves the avatar and the flycam camera unintentionally.

## Done (2026-11-27)

The reference has no keyboard flycam mode at all: its flycam is driven by the
6-DOF device (`LLViewerJoystick::moveFlycam` *instead of* `moveAvatar`), and
the keyboard stays on `keys.xml`'s avatar modes. Our flycam also flies on the
keyboard, so the two sets are now split by action rather than by an early
return:

- **New `Action::Flycam*` actions** (forward, back, left, right, up, down) are
  what `drive_flycam` reads. Only the flycam profile binds them, on the WASD
  cluster and `E` / `C`.
- **The flycam profile keeps the avatar keys on the avatar**: the arrows walk
  and turn it, `PageUp` / `PageDown` fly it, `F` toggles flight, all through the
  ordinary `Move*` / `ToggleFly` actions. The two key sets are disjoint, so no
  key moves both. `Shift` is shared as a modifier only: it runs the avatar and
  speeds the camera, each only while that one is moving. `PageUp` / `PageDown`
  moved from the camera to the avatar.
- **`drive_avatar_controls` no longer returns early in flycam.** The
  SpaceNavigator contributes nothing to the avatar there, since it flies the
  camera. A flying avatar with no key held still sends `FLY` alone and hovers,
  so entering the flycam does not drop it. The seated branch now runs in flycam
  too, which tidies a seated avatar's `flying` flag (the edge noted above).

Tests: `input_action` checks that the avatar keys resolve to avatar actions in
flycam, that `W` / `E` resolve per mode, and that the camera and avatar key sets
are disjoint. The world-tier `the_avatar_keys_drive_the_avatar_in_flycam` checks
`AT_POS`, a turn with its rotation send, `UP_POS` and `FLY` on the wire in
flycam, and that `W` sends nothing. The e2e stage
`e2e_movement::the_arrow_walks_the_avatar_in_the_flycam_and_w_flies_only_the_camera`
checks that `W` flies the camera at least 4 m while the avatar stays put, and
that the arrow then walks the avatar 3 m with the camera still a flycam. It
passed on the fake grid, live on OpenSim (Northeast Region 70/180) and live on
aditi.
