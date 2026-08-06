---
id: viewer-object-ping-forward-interpolation
title: Nudge dead-reckoned objects forward by half the ping (sPingInterpolate parity)
topic: viewer
status: ready
origin: raised reviewing viewer-physical-object-motion-not-smooth reference parity (2026-08-06)
refs: [viewer-physical-object-motion-not-smooth]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's `LLViewerObject::interpolateLinearMotion` finishes with a
**ping-compensation** step (`sPingInterpolate`, on by default): on each object
update it nudges the predicted position **forward** by half the circuit ping,
`new_pos += 0.5 * time_dilation * (ping + frame_dt) * velocity`, so a moving
object renders where it *is now* rather than where it was when the packet was
sent (`indra/newview/llviewerobject.cpp`, the `sPingInterpolate` block). Our
`drive_physical_objects` / `drive_avatar_motion` port the rest of
`interpolateLinearMotion` (the velocity+accel extrapolation, phase-out taper,
geometric clamps, region-crossing cap) but **not** this ping nudge — so our
dead-reckoned objects sit a fraction of a ping-time behind the reference's.

We already have the input: the circuit round-trip time is measured
(`StartPingCheck` / `CompletePingCheck` → `Event::Ping`), so this is a small
parity-completion — apply the half-ping forward offset to the predicted position
using the current region time dilation, matching the reference. Scope it to the
same two dead-reckon drivers (object + avatar).

Interaction to respect: the residual ease
([[viewer-physical-object-motion-not-smooth]]) smooths the per-update
correction; the ping nudge shifts the *prediction target*, so apply it inside
the prediction (before the residual is re-aimed), not as a separate render
offset, or the two will fight. Low priority — a currency refinement, not a
smoothness fix (and the dominant vehicle-smoothness win was the seat-locked
camera, [[viewer-sit-camera-vehicle-frame-lag]], not the extrapolation).
