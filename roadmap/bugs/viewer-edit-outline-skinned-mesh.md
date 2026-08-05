---
id: viewer-edit-outline-skinned-mesh
title: Edit-selection outline can't shell a skinned / animesh object
topic: viewer
status: bugs
origin: user report during aditi verification (2026-08-05)
refs: [viewer-object-selection-core, viewer-animesh-transparent-box-shell]
---

Context: [context/viewer.md](../context/viewer.md).

The edit-selection highlight (`apply_selection_highlight`,
`edit_selection.rs`) draws an **inverted-hull shell** over every selected face
mesh: a clone of the face's `Mesh3d` with a front-face-culled outline material,
inflated by an entity-`Transform` scale so only the rim shows.

That approach is incompatible with a **skinned** face (an animesh, or any
rigged in-world mesh):

- The cloned mesh carries `JOINT_INDEX` / `JOINT_WEIGHT`, so the renderer
  specialises it into the **skinned** pipeline (`pbr_alpha_blend_mesh_pipeline`
  / `skinned_mesh_layout`), but the shell has no `SkinnedMesh` component, so it
  is handed the non-skinned (`model_only`) mesh bind group. The mismatch is a
  wgpu validation error — and the render-error handler **quits the app** on it.
  Exact message: *"BindGroup 'model_only_mesh_bind_group' … is not compatible
  with … RenderPipeline 'pbr_alpha_blend_mesh_pipeline' … Expected entry with
  binding 1 not found in assigned bind group layout."* Reproduced on aditi by
  opening the build floater on an animesh; localhost/OpenSim did not have a
  rigged selected mesh so it never bit there.
- The inflate is an entity-`Transform` scale, which skinning **ignores** (the
  GPU deforms vertices from the joint matrices, not the entity transform), so
  even with a matching `SkinnedMesh` the shell would sit coincident with the
  mesh rather than forming an inflated rim.

**Current mitigation (shipped 2026-08-05):** `apply_selection_highlight` skips
the shell for a face that has a `SkinnedMesh`, which removes the crash. A
selected animesh then keeps only the transform-gizmo handles (move arrows /
rotate rings, and the Stretch tool's bounding box) — it loses the silhouette
glow that a plain prim gets.

**The fix to design:**

- Check how **Firestorm** draws the edit selection outline / highlight on a
  rigged or animesh object — a skinning-aware silhouette, a bounding box, or
  something else — and match it (`llselectmgr.cpp` `renderSilhouettes` /
  `LLSelectNode::renderOneSilhouette`, and how it handles rigged attachments /
  `LLVOVolume` animated objects).
- Then either (a) a **skinning-aware inverted-hull** outline (a vertex shader
  that inflates along the *posed* normal, sharing the mesh's `SkinnedMesh` so it
  follows the animation), or (b) a static **bounding-box** marker at the
  object's placed pose (skinning-independent, approximate — bounds where the
  object sits rather than hugging the deformed mesh). (a) is faithful; (b) is
  the cheap clear marker.

Related: [[viewer-animesh-transparent-box-shell]] (a separate animesh box-shell
artifact) is a different symptom in the same skinned-object rendering area.
