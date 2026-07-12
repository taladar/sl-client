---
id: viewer-r2
title: Fix rigid eyeball placement (was P15.5)
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R2. Fix rigid eyeball placement (was P15.5).** The rigid eyeballs read
too low / recessed in the socket (a see-through gap above the eyeball). The
perception-vs-measurement gap was **real**, with two independent causes, both
now fixed (confirmed live on OpenSim — the eyes seat cleanly with white sclera
and visible irises):

- **Base-mesh skinning joint mapping (the actual placement cause).** Second
  Life base parts store one weight float per vertex whose integer part indexes
  the reference viewer's **`mJointRenderData`** list — a depth-first skeleton
  walk with each group's base ancestor prepended
  (`LLAvatarJointMesh::setupJoint`; `avatarSkinV.glsl`:
  `mix(palette[floor(w)], palette[floor(w)+1], fract(w))`) — **not** the
  mesh's own `joint_names` table. Our decoder mapped it into `joint_names` and
  clamped, so the head's `[mHead, mNeck]` names sent every face vertex (weight
  `2.0`) to `mNeck` instead of `mHead`. It renders correct at rest (the
  inverse bind-pose cancels it) but under skeletal deformation the whole face
  was dragged by the
  *neck* while the rigid eyeball (correctly on `mEyeLeft` → `mHead`) was not —
  the divergence. Fixed by keeping the raw weight index (`sl-avatar`
  `split_weight`) and rebuilding the render list (`sl-client-bevy`
  `base_mesh_skin` / `joint_render_data`). Also corrects the whole base body's
  shape under deformation, not just the eye.
- **Missing eye sclera (the "untextured" half).** Our client-side eye bake
  carried only the iris layer, so the eyeball read as a featureless blob
  (easily misread as misplaced). Added the reference `eyes` layer-set's white
  sclera base (`eyewhite.tga`) under the iris — part of the broader static-TGA
  bake layers below.
Note: the *rigid* eyeball itself has **no** placement offset in Firestorm
(`setMesh` uses the `.llm` origin, pinned to `mEyeLeft`; eye tracking is
rotation-only) — the fix was upstream, in the skeleton/skinning.
