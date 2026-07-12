---
id: viewer-r13
title: Rest-visible spike under one shoulder
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R13. Rest-visible spike under one shoulder** (`sl-client-bevy` /
`sl-avatar`). With the shape correct (**R12**), a triangular flap of geometry
poked out under the avatar's **right** armpit **at rest** (the left armpit was
clean — the asymmetry was the tell). The premise above was **wrong on two
counts**: it *was* skinning, and it is *not* invisible at rest, because the
skeletal-deformation visual params move the joints off the bindpose the base
part's inverse-binds assume, so a wrongly bound vertex spikes wherever the
rest deformation is non-trivial (the armpit). **Root cause:** the base-mesh
joint-render-data list (`BevySkeleton::joint_render_data`, from **R2**)
prepended each skin joint's **direct parent** as its ancestor; the reference
viewer prepends the nearest **base-skeleton** ancestor
(`getBaseSkeletonAncestor`, SL-287), *skipping* the extended (Bento) joints
(`mSpine1`..`mSpine4`) that sit between `mTorso`/`mChest` and `mPelvis`.
Including `mSpine2`/`mSpine4` inserted two extra render-list slots, shifting
every later weight index by two — so a right-armpit vertex authored for
`mChest` (raw weight `10.1`) resolved to `mElbowLeft` and was dragged across
the body, and the whole left arm (weights `7`–`8`,
`mShoulderLeft`/`mElbowLeft` in the reference list) bound to
`mChest`/`mCollarLeft`. **Fix:** a
`JointSupport` enum (`Base`/`Extended`) parsed from the `support` attribute in
`sl-avatar`'s skeleton, carried into `BevySkeleton`, and a `base_ancestor`
walk that skips non-base ancestors — the render list now matches the reference
exactly and the skin displacements are symmetric. Confirmed live (own avatar,
local OpenSim) top-down: the flap is gone. Because the whole arm chain was
wrongly bound, this is expected to also fix — or substantially reduce —
**R11** (the animation-time arm distortion), which should be re-checked next.
New
debug affordances added for this class of work (kept): `SL_VIEWER_CAMERA_*`
(`ORBIT_DEG` / `ELEV_DEG` / `DISTANCE` / `TARGET_Z`) orbit the login-framing
camera so the offline screenshot harness can capture a hidden spot, and
`SL_VIEWER_LOG_AVATAR_GEOMETRY` logs per-part morph- and skin-displacement
outliers (with each vertex's render-slot → joint name) — the tool that
localised this. Surfaced by the R12 Firestorm side-by-side.
