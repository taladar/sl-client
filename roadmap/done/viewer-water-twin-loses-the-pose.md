---
id: viewer-water-twin-loses-the-pose
title: The waterline split's twin of a rigged face drew at no pose at all
topic: viewer
status: done
origin: found while porting the edit-selection outline to skinned faces
  (2026-09-08)
refs: [viewer-skinned-bind-group-quits-on-rez, viewer-edit-outline-skinned-mesh,
  viewer-straddling-transparency-oit]
---

Context: [context/viewer.md](../context/viewer.md).

`water_clip` gives a translucent face that straddles the waterline a **twin**
entity drawing the same mesh clipped to the other side.
[[viewer-skinned-bind-group-quits-on-rez]] made the twin clone the face's
`SkinnedMesh`, which stopped the wgpu validation error that quit the viewer. It
did not make the twin *posed*.

Bevy's `SkinnedMesh` is two things here, and only one of them was cloned:

- the component that makes Bevy allocate the entity a palette at all (without
  it, the validation error);
- and the **pose** written into that palette. This viewer skins on the GPU: pass
  D overwrites each entity's palette in `SkinUniforms`, addressed by the
  `GpuSkinBinding` that entity carries, and a GPU-posed rig's `SkinnedMesh`
  binds every slot to a shared **placeholder** joint precisely because nothing
  is meant to read it.

So the twin, carrying the skin but no binding, fell through to Bevy's own
`extract_skins`, which gathered those placeholder joints and drew the underwater
half of every rigged translucent face collapsed — invisible, which is what the
split exists to prevent. Its `Aabb` was wrong for the same reason:
`apply_gpu_avatar_bounds` keys on the binding too, so the twin kept a
meaningless bind-pose bound rather than the read-back posed one.

## The fix

A shared marker, `SkinPoseTwin { source }` in `sl-viewer-world-api`:
*this entity draws a second copy of that entity's skinned geometry.*
`gpu_avatars::stage::sync_skin_pose_twins` copies the source's `GpuSkinBinding`
onto the twin (and takes it away, with `ExternallyPosedSkin`, when the source
loses its own), before the stage that reads every binding. The marker lives in
the API crate because neither the water layer nor the build tool may depend on
the avatar layer.

The same mechanism poses the build tool's selection outline on a rigged face —
see [[viewer-edit-outline-skinned-mesh]], where it was found.
