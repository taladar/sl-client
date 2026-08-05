---
id: viewer-sit-target-and-stand-button
title: Sit target ignored on sit, and no Stand button while seated
topic: viewer
status: bugs
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
