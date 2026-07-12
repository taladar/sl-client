---
id: viewer-r17
title: Shoe height / heel offset not applied to avatar placement
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R17. Shoe height / heel offset not applied to avatar placement**
(`sl-client-bevy-viewer`). Surfaced during the P20.2 aditi session: the worn
**shoe** wearable's height adjustment — the heel / platform offset that raises
the avatar so its feet rest on the ground — was not taken into account, so a
shoe-wearing avatar sank into or floated above the ground. The body was
planted only by the fixed pelvis rest height (P13.2), ignoring the shoe. The
shoe height is **already a skeletal deformation** we resolve: the `Shoe_Heels`
(id 197, driven by the transmitted `Heel Height` id 198) and `Shoe_Platform`
(id 502) `param_skeleton`s offset `mFootLeft` / `mFootRight` downward in Z, so
the reference viewer's `computeBodySize` folds that offset into
`mPelvisToFoot` (`- foot.z * ankle_scale.z`) and stands the avatar taller.
**Fix:** a per-agent `pelvis_lift`, computed from the resolved deformations as
`-offset(mFootLeft).z * (1 + scale(mAnkleLeft).z)` (clamped ≥ 0 — a shoe only
ever raises), is added to the pelvis rest height when planting the body root;
`apply_avatar_appearance` re-plants an already-spawned, possibly-stationary
body the moment its shoe lift changes (a disjoint anchor query) rather than
waiting for its next position update. Unit-tested
(`shoe_offset_lifts_the_body`); not visually confirmed against a shod avatar
this session (the default own avatar wears no shoes and no second avatar was
in view). Reference: `LLAvatarAppearance::computeBodySize` `mPelvisToFoot`,
`avatar_lad.xml` `Shoe_Heels` / `Shoe_Platform` `param_skeleton`.
