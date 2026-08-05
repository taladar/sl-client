---
id: viewer-sit-target-and-stand-button
title: Sit target ignored on sit, and no Stand button while seated
topic: viewer
status: done
origin: user report during aditi verification (2026-08-05)
refs: [protocol-38]
---

Context: [context/viewer.md](../context/viewer.md).

Two sitting defects seen on aditi:

- **Sit target not respected.** When sitting on an object with a scripted /
  defined sit target (`llSitTarget`, or the `AvatarSitResponse` /
  `SitTransform` offset the simulator returns), the avatar is not placed at the
  target position / rotation. The protocol side landed as [[protocol-38]] (the
  reply carries the seat offset + rotation), so the viewer is decoding the
  transform but not applying it to the seated avatar's placement — check that
  the `SitTransform` offset/rotation is composed onto the avatar relative to the
  seat object (the reference's `LLVOAvatar::sitOnObject` /
  `LLAgent::setSitCamera`, and how the seat's offset combines with the object's
  frame).

- **No Stand button while seated.** When the avatar is sitting there is no
  visible **Stand Up** control, so there is no in-UI way to stand (the reference
  shows a "Stand Up" button, typically in the bottom toolbar / a stand
  affordance while seated). Wire a Stand control gated on the seated state
  (`Command::StandUp`; the sit state is tracked — see the `SitState` typestate)
  so standing does not depend on a menu-only path.

Both are viewer-side (the sit request / response protocol works); the fix is
applying the seat transform to the rendered avatar and surfacing the
seated-state Stand affordance in the UI.

## Implemented (2026-08-05)

Scope grew, at the user's direction during implementation, to a faithful sit
placement + camera pass (overlapping [[viewer-sit-stand-actions]], whose camera
/ mouselook / stand-UI parts this closes):

- **Seated placement (self *and* others), as a parenting relationship.** A
  seated avatar's `ObjectUpdate` carries a non-zero `ParentID` and a
  seat-relative pose; the visible anchor was ignoring it (placed at those
  offsets as if region-local). Now `AvatarState` tracks seated avatars and
  `place_seated_avatars` composes each one's seat-relative pose onto its seat
  object's live world transform every frame, so the avatar rides a moving seat
  (a boat full of avatars rides together). The dead-reckoner skips a `Seated`
  anchor. Faithful to `LLVOAvatar::sitOnObject` (seat-relative `rel_pos` /
  `rel_rot`, **no** standing capsule-centre correction while seated — so the R17
  shoe / heel offset is excluded, matching the reference's
  `!(isSitting() && getParent())` branch). The sit offset targets the avatar
  **root** (hips / `mPelvis`), so the anchor (our body root, which sits
  `pelvis_local_z` below the pelvis) is dropped by that pelvis height along the
  sit orientation's up (`drop_to_hips`, per-avatar `seat_drops`) — the
  reference's `mRoot` *is* the pelvis, so it needs no such drop; ours does.
  Without it a seated avatar floats ~1 m above the seat (found live on aditi).
- **Scripted sit camera + forced mouselook** from the `AvatarSitResponse`
  (`SitResult`): the third-person camera rides the seat at the script's
  `llSetCameraEyeOffset` / `llSetCameraAtOffset` (the reference's `setSitCamera`
  / the sit-camera branch of the camera-target math, enabled past the 1 mm
  offset threshold), and `ForceMouselook` drops into mouselook on sit and
  restores third person on stand. New `sit_camera` module + `SitCamera`
  resource.
- **No-sit-target offset.** The `AgentRequestSit` offset for a "Sit Here" now
  matches the reference: a port of `LLAgentCamera::calcFocusOffset`
  (`sit_offset.rs`) — project the click onto the prim's most-camera-facing axial
  plane through its centre, clip to the bounding box, bias toward the surface by
  camera distance — sent in **region axes** (the sim adds it to the prim's
  absolute position unrotated: OpenSim `SendSitResponse`
  `pos = part.AbsolutePosition + offset`), not the old prim-local surface point.
  The simulator's own flat-surface search / "There is no suitable surface to sit
  on" alert is sim-side and already surfaces through our AlertMessage path.
- **Stand Up control** in a reserved **leading** slot of the bottom toolbar,
  unified with the relocated **Stop flycam** button (the old top-centre bar was
  folded in here) — the reference's combined `LLPanelStandStopFlying`. Sitting
  shows Stand, flycam shows Stop flycam, at most one, sitting first. The slot is
  fixed-width (balanced by a trailing spacer) so it never reflows the centred
  buttons and never intrudes on the bottom-left conversation dock. New
  `stand_stop_button` module; `flycam_ui` removed.

Follow-ups noted: [[viewer-hover-height]] (hover offset), and the bottom-right
quick-preferences panel ([[viewer-quick-preferences]]).
