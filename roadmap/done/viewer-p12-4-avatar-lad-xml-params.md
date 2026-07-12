---
id: viewer-p12-4
title: avatar_lad.xml params
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 12 — `sl-avatar`: skeleton & base body (pure crate)
---

Context: [context/viewer.md](../context/viewer.md).

**P12.4. `avatar_lad.xml` params.** `params.rs`: parse the visual-param
table — id, group, min/max/default, and each param's effect (`param_morph`
mesh delta ref, `param_skeleton` bone scale/offset, driver→driven links).
Produce a `VisualParams` model that maps an `AvatarAppearance.visual_params:
Vec<u8>` (quantized 0–255, viewer order) onto typed param values. **Done:**
`VisualParams::from_xml` collects every `<param>` anywhere in the document
(skeleton / mesh / layer-set / driver sections), deduplicating by id (last
definition wins, mirroring `addVisualParam`'s map overwrite) and sorting by
ascending id. Each `VisualParam` carries `{ id, group, name, label, wearable,
sex, min, max, default, effect }`, where `ParamEffect` is one of `Morph`
(target resolved later by name in the base-mesh morph table),
`Skeleton(Vec<BoneOffset>)` (per-bone `scale` + optional `offset`),
`Driver(Vec<DrivenParam>)` (each with the `min1/max1/max2/min2` trapezoid
thresholds, absent ones defaulting to the driver's own bounds), `Color`
(RGBA ramp) or `Alpha` (bake inputs kept so they still occupy wire slots),
or `None`. `ParamGroup::is_transmitted` selects the wire subset (Tweakable
`0` + TransmitNotTweakable `3`); the reference viewer packs those **sorted by
id** because it iterates a `std::map<S32, LLVisualParam*>` in key order, so
`VisualParams::transmitted()` is exactly the wire order and
`map_appearance(&[u8])` dequantizes byte `i` against the `i`-th transmitted
param via Firestorm's `U8_to_F32` ramp (with its snap-to-zero step), leaving
short-vector tail params at their default. Committed fixture
`mini_params.xml` (one param of each effect type + a non-transmitted group-1
param, ids out of document order to exercise the id sort); `cargo test -p
sl-avatar` (9 new tests). LIVE-VALIDATED against the real (uncommitted)
Firestorm `avatar_lad.xml`: 672 distinct params, **253 transmitted** (the
known SL wire count), every param resolving to a recognized effect
(morph 223 / skeleton 83 / driver 164 / color 108 / alpha 94, none 0); first
wire ids `Big_Brow`(1)/`Nose_Big_Out`(2)/`Broad_Nostrils`(4)…, and the
`Male_Skeleton`(32) param carrying 22 skeletal bones.
