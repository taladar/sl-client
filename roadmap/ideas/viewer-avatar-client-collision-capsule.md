---
id: viewer-avatar-client-collision-capsule
title: Client-side avatar collision capsule (if client physics ever needs avatars)
topic: viewer
origin: camera-collision / avatar-physics-flag discussion (2026-08-15)
refs: [viewer-physics-static-prim-colliders]
---

Context: [context/viewer.md](../context/viewer.md).

Avatars deliberately carry **no** avian collider today: their physical presence
(avatar-avatar bumping, being pushed, walking into things) is simulated
**server-side**, and the viewer just renders the server-authoritative position
(`drive_avatar_motion`). `is_physical_root` explicitly excludes `pcode::AVATAR`
so the avatar object never becomes a kinematic "physical prim" with a stray
cuboid collider (that was the camera-into-the-head bug —
[[viewer-physics-static-prim-colliders]]). Camera collision also excludes
avatars on purpose (the reference camera does not pull in for them).

But **if** the viewer ever runs a client-side simulation that should *interact*
with avatars — e.g. Phase 34 avatar cloth/body physics colliding against the
wearer's own body, a client-side "push" effect, or client-predicted
avatar-object contact — it would need an avatar collision shape
**in the client physics world**.

Direction, if pursued: give the avatar a purpose-built **capsule** collider (SL
uses a capsule for the avatar), sized from the avatar's height/width and
positioned each frame from the avatar pose (the `avatars.rs` motion path), on
its **own collision layer** so it never leaks into camera collision or the
"objects near X" set. **Not** the accidental prim-cuboid path — that is the
anti-pattern this avoids. Note this likely only matters if Phase 34 lands on
avian dynamic bodies; flexi (Phase 32) went with a bespoke solver instead
([`crate::flexi`]), so avatar cloth may well do the same and never need this.

Parked in `ideas/` — no consumer today; revisit when Phase 34 (avatar
cloth/body physics) is scoped.
