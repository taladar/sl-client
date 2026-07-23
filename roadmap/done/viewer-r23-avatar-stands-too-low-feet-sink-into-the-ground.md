---
id: viewer-r23
title: Avatar stands too low — feet sink into the ground
topic: viewer
status: done
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

**Fixed (2026-07-23).** `BevySkeleton::body_size_metrics`
(`sl-client-bevy/src/avatars.rs`) ports `computeBodySize` verbatim —
`pelvis_to_foot` and `body_size_z` from the deformed chain's current local
positions and scales, with rig joint-position overrides (and their scale
lock) resolved exactly as `deformed_world_matrices` does; unit tests pin the
rest values (0.979 / 1.707) with a real-valued chain fixture. The viewer
plants the body root at `reported_z − root_drop` with
`root_drop = 0.5·body_size_z − pelvis_to_foot + pelvis_local_z − hover`
(`root_drop_from_metrics`, `avatars.rs`), the hover being the transmitted
`Hover` shape param (id 11001); soles land at the reference's
`reported_z − 0.5·body_size_z + hover`. The per-agent drop is resolved in
`apply_avatar_appearance` and re-plants a standing body by delta, replacing
the R17 `pelvis_lift` mechanism entirely.

Deliberate deviations from the plan above, both toward reference fidelity:

- **`shoe_lift` did *not* stay a separate additive term** — the reference
  has no such term: the shoe params' `mFoot*` offset flows into the metrics'
  foot term (both quantities grow by the lift, so the root rises by *half*
  of it), which the old additive term double-counted (and in the wrong
  direction: it lowered the root). A unit test pins the folded behaviour;
  [[viewer-r17a]] remains the live visual check.
- **The un-rigged fallback (placeholder sphere) was left at the reported
  position** — under the corrected reading the wire Z is the capsule
  *centre*, which is exactly where a whole-avatar stand-in sphere belongs;
  the "needs the same model" note assumed the old pelvis reading.

The region-side hover preference (`getHoverOffset()`, the `AgentPreferences`
capability / `llSetHoverHeight`) is still not ingested — only the shape's
Hover param is applied.

**Live-check finding (2026-07-23): the first run floated the own avatar a
few cm above the ground** (terrain and ramp) instead of sinking. Root
cause: the advertised `AgentSetAppearance` size was a hardcoded `z = 1.9`
(`bake_publish.rs` `AVATAR_SIZE`), and OpenSim's `SetSize` takes the
client's word for it — the capsule (and hence the reported capsule-centre
Z) was sized for a 1.9 m avatar while the render used the true
`body_size_metrics` height; the avatar floats by half the difference.
Fixed by advertising `computeBodySize` of exactly the published
`visual_params` (`advertised_size`), the reference's `mBodySize` (which
Firestorm also sends hover-less on OpenSim). Residual expectations: on
SL/aditi no `AgentSetAppearance` publish runs (server bakes), so the
server-side height comes from the account's real viewer and any
account-level hover preference — which we do not ingest yet — can still
show as a small offset there.
