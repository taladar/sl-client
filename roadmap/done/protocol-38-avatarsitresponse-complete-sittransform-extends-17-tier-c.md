---
id: protocol-38
title: AvatarSitResponse complete SitTransform (extends #17, Tier C)
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**38. `AvatarSitResponse` complete `SitTransform` (extends #17, Tier C). ✅
Done.** `Event::SitResult` (`session.rs`) surfaced only `sit_object`,
`autopilot`, and `sit_position`, dropping the rest of `SitTransform`. Added and
populated the remaining four fields: **`sit_rotation`** (the seated
orientation — which way the avatar faces once seated), `camera_eye_offset` /
`camera_at_offset`
(scripted-sit cameras, `llSetCameraEyeOffset`/`…AtOffset`; the zero vector when
the seat's script sets no custom camera), and **`force_mouselook`**
(vehicles/weapons HUDs force the avatar into mouselook on sit). For consistency
with the codebase's geometry convention (`ObjectMotion`, the `position: Vector`
events) `sit_position` was also promoted from a bare `(f32, f32, f32)` tuple to
`sl_types::lsl::Vector`, and the new offsets/rotation use `Vector`/`Rotation`.
Re-exported through both runtimes (the `tokio_login_hold_logout` /
`bevy_login_hold_logout` examples now log the orientation and mouselook flag).
Covered by the `sl-proto` `sit_request_completes_on_response` lifecycle test,
extended to assert all four new fields round-trip (the quaternion's `s` is
reconstructed from the wire's `x/y/z`, so it is compared with an epsilon).
*Test: local OpenSim (a scripted sit target); the decode is unit-tested.*
