---
id: viewer-edit-outline-skinned-mesh
title: Edit-selection outline can't shell a skinned / animesh object
topic: viewer
status: done
origin: user report during aditi verification (2026-08-05)
refs: [viewer-object-selection-core, viewer-animesh-transparent-box-shell,
  viewer-skinned-bind-group-quits-on-rez, viewer-water-twin-loses-the-pose,
  viewer-mesh-objects-outlined-by-a-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The edit-selection highlight (`apply_selection_highlight`, `edit_selection.rs`)
drew an **inverted-hull shell** over every selected face: a clone of the face's
`Mesh3d` with a front-face-culled outline material, inflated by an entity
`Transform` scale so only the rim shows. A **skinned** face (an animesh, or any
rigged in-world mesh) could not wear it, in two independent ways:

- the shell shares the face's mesh asset, so Bevy specialises it into the
  **skinned** pipeline while taking the bind group from the *entity* — a shell
  without a `SkinnedMesh` gets the model-only bind group, which is the wgpu
  validation error the render handler quits the viewer on (the same defect as
  [[viewer-skinned-bind-group-quits-on-rez]], found later in a second path);
- and the inflate is an entity-`Transform` scale, which a skinned draw
  **ignores** — vertices are placed from the joint palette. Even a correctly
  skinned shell would sit exactly on the face rather than around it.

The shipped mitigation (2026-08-05) skipped the shell for a `SkinnedMesh` face,
which removed the crash and left a selected animesh with only the gizmo handles.

## What the reference actually does

Not an inverted hull, and not a silhouette either.
`LLSelectNode::renderOneSilhouette` **returns early** for a mesh object
(`vobj->isMesh()` — the `SL-10194` removal of `renderOneWireframe`), and
`LLSelectMgr::renderSilhouettes` routes every mesh object — rigged or not — to
`renderMeshSelection_f` instead, which draws the selected faces as a
**wireframe**: `glPolygonMode(GL_FRONT_AND_BACK, GL_LINE)`,
`LLFace::renderOneWireframe` at line width 5 with `glPolygonOffset(3, 3)`, in
the same parent-yellow / child-blue silhouette colours (and doubled brightness).
For a rigged drawable it first re-skins the volume (`updateRiggedVolume(true)`),
so the lines follow the **pose**.

So the faithful highlight for a rigged face is a posed wireframe. Neither
candidate this task listed — a skinning-aware inverted hull, or a static
bounding box — is what the reference draws.

## The fix

`selection_wireframe::wireframe_mesh` derives the overlay from the face's own
mesh: the unique triangle edges as a `PrimitiveTopology::LineList`, **every**
vertex attribute kept (the shader is compiled against the face's own layout, and
the joints are what make the overlay skin), with the positions lifted along
their normals — the port of the reference's polygon offset, carried in the
geometry because the entity transform is the one lever a skinned draw ignores.

The overlay carries the face's `SkinnedMesh` **and** a `SkinPoseTwin`. The
second is load-bearing: this viewer skins on the GPU and writes each entity's
palette from a `GpuSkinBinding` the avatar layer keeps, so an overlay with only
the cloned `SkinnedMesh` falls through to Bevy's own skin extract, reads the
placeholder joints a GPU-posed rig binds, and draws collapsed. The marker lives
in `sl-viewer-world-api` (neither the build tool nor the water layer may depend
on the avatar layer) and `gpu_avatars::stage::sync_skin_pose_twins` copies the
binding across.

Two further paths shared the same mesh handle with no skin and would have hit
the same crash on a rigged face — neither had the 2026-08-05 mitigation:

- `apply_drag_hover_highlight`, the green/red outline while an inventory item is
  dragged over an object (it now draws the same wireframe);
- `apply_face_cursor_highlight`, the Select Face grid cursor (it now clones the
  skin and the pose marker; being coplanar with the face, skinning it
  identically is exactly right).

Line **thickness** is the one divergence left: `wgpu` has no line-width state,
so the rim is one pixel where the reference's is five.

## Verified

On aditi (2026-09-08): a rigged object selected with the build tool wears the
white primary outline, following it; an ordinary prim still wears the hull, so
the unchanged path is unchanged. The **drag-drop** side could not be seen on a
rigged face for want of an animesh the test avatar may modify — dragging over
someone else's animesh is refused one gate earlier, at
`resolve_hover_entity`, which is the reference's own rule (only an object the
drop would be accepted by is highlighted). Its spawn path is the one the
selection outline uses, and both halves now have unit coverage.

Chasing that "nothing happens at all" report is what added
`inventory_drag::DRAG_HOVER_LOG_TARGET` (`sl_viewer::drag_hover`): the chain
crosses three tiers through three resources, so a missing outline had half a
dozen equally plausible causes and nothing on screen told them apart. Each stage
now names itself, deduplicated, and one live run settled it.

Filed alongside: [[viewer-water-twin-loses-the-pose]] (the waterline split's
twin had the same missing pose, fixed with the same marker) and
[[viewer-mesh-objects-outlined-by-a-shell]] (the reference wireframes *unrigged*
mesh objects too; ours still shells them).

Related: [[viewer-animesh-transparent-box-shell]] is a different symptom in the
same skinned-object rendering area.
