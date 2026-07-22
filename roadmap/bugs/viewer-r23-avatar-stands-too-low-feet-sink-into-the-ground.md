---
id: viewer-r23
title: Avatar stands too low — feet sink into the ground
topic: viewer
status: bugs
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R23. Avatar stands too low — feet sink into the ground.** Our viewer
renders the avatar with its feet buried below the terrain surface; in
Firestorm the same avatar's feet rest *on* the ground. The avatar root is
placed at too low a Z by roughly the ankle-to-sole height, so the whole body
is offset downward. Cosmetic but consistently visible. **Open** (root cause
found, fix pending).

**Root cause (found 2026-07-22): the wire position is the capsule/bounding-box
centre, but we treat it as the pelvis.**

- Our `body_root_transform` (`sl-client-bevy-viewer/src/avatars.rs`) places
  the body root (sole level) at `reported_z − pelvis_height − shoe_lift`,
  where `pelvis_height()` (`avatar_assets.rs`) is the **fixed rest** local Z
  of `mPelvis` (1.067 m, `avatar_skeleton.xml`), unscaled by the shape — i.e.
  it puts the *pelvis* exactly at the reported Z.
- The wire Z of an avatar is **not** the pelvis. OpenSim reports the physics
  capsule **centre**: `pos.Z = ground + 0.5 · Appearance.AvatarHeight`
  (`ScenePresence.cs` `localAVHalfHeight`, verified ~line 1458; the ubOde
  capsule tracks its centre). The reference viewer assumes the same:
  `LLVOAvatar::updateCharacter` applies
  `root_pos.z −= 0.5·mBodySize.z − mPelvisToFoot` — commented "correct for
  the fact that the pelvis is not necessarily the center of the agent's
  physical representation" — then adds `getHoverOffset()` / `AVATAR_HOVER`.
  Net: reference soles land at `reported_z − 0.5·mBodySize.z (+ hover)`;
  `mPelvisToFoot` cancels out of the sole height.
- So our sink is `1.067 − 0.5·mBodySize.z`, which for a ~1.9–2.0 m shape is
  the observed ~7–12 cm ("roughly ankle-to-sole"), and varies with the shape:
  shorter avatars sink more, since our 1.067 is constant while the true
  correction is half the **shape-scaled** body height.
- Both reference quantities come from `LLAvatarAppearance::computeBodySize`
  (`llavatarappearance.cpp:533`): `mPelvisToFoot = hip.z·pelvis_scale −
  knee.z·hip_scale − ankle.z·knee_scale − foot.z·ankle_scale` (≈0.979 at
  rest — note LL's sign quirk on the hip term, and that it *includes* the
  foot-below-ankle segment), and `mBodySize.z = mPelvisToFoot + √2·skull.z·
  head_scale + head.z·neck_scale + neck.z·chest_scale + chest.z·torso_scale +
  torso.z·pelvis_scale` (≈1.707 at rest) — every term shape-scaled.

**Fix direction:** plant the sole at `reported_z − 0.5·mBodySize.z (+ hover)`
with `mBodySize.z` computed per avatar from the shape-scaled joint chain like
`computeBodySize` (and add the missing hover offset), instead of subtracting
the fixed rest `pelvis_height`. Related notes: the un-rigged fallback path
(`avatars.rs` ~1574) applies **no** offset at all and needs the same model;
the foot-IK's relative-displacement design (`locomotion_ik.rs`, the
"deliberate deviation" note) is insensitive to absolute root error and
survives this fix unchanged — but its comment describing the root placement
should be updated with it. `shoe_lift` (R17) stays a separate additive term.
