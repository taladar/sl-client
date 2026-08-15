---
id: viewer-flycam-avatar-movement-keys
title: Avatar movement keys do nothing in flycam mode
topic: viewer
status: ready
origin: user report during GPU-avatar crowd testing (2026-08-14)
---

Context: [context/viewer.md](../context/viewer.md).

In **flycam** camera mode the avatar movement keys — arrow keys (walk / turn),
and by extension the up/down (fly) keys (PgUp / PgDn) and the other avatar
movement bindings — do nothing. The reference keeps the avatar controllable in
this mode: the flycam moves the *camera* independently, but the movement keys
still drive the *avatar*.

Likely an input-routing gate: the avatar-movement system is probably suppressed
while `CameraMode::Flycam` is active (or the flycam consumes those keys for the
camera), so the avatar's `AGENT_CONTROL_*` movement is never sent. The flycam
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
