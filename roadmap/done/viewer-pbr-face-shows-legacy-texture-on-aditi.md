---
id: viewer-pbr-face-shows-legacy-texture-on-aditi
title: A prim given a PBR material from the Build window sometimes keeps drawing its plywood
topic: viewer
status: done
origin: viewer-realtime-mirrors (2026-11-10)
refs: [viewer-realtime-mirrors, viewer-build-probe-animesh-controls]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

In three of four aditi runs of `a_mirror_reflects_the_avatar_on_second_life`
the pane — a fresh box given the blank PBR material with the Texture tab's
New, then metallic 1 / roughness 0 — was drawn with its legacy plywood in the
final screenshot. The grid held the material on every face and the override
(`{'rf':0}`, read back with `sl-repl`), so the data was right; the fourth run
drew polished metal with nothing changed but timing.

The material pipeline has since gained debug lines (`render material …
registered / assigned`, `material … decoded`, `recomposing … (decoded)`) and
warnings for a fetch that fails or comes back empty, which used to be dropped
silently. The run that drew metal showed the material assigned, decoded and
recomposed after each update.

Suspects: the Texture tab's Blinn-Phong preview (`apply_blinn_phong_hide`)
hiding the faces while the tab was on Material mode and the restore losing a
race with a parked Blinn-Phong texture patch; or a re-tessellation of the
faces after the recomposition re-describing them with the legacy material.
Next step: run the stage again with `sl_viewer_world_objects::materials=debug`
until it reproduces, and read the order of hide / restore / rebuild.

## Resolution (2026-11-15)

Not reproduced: four aditi runs of the stage with
`sl_viewer_world_objects::materials=debug` and a detach log all drew polished
metal. Reading the material and object code for what could leave a PBR face
drawing its Blinn-Phong layer with the data right turned up two races, each of
which ends in exactly that state: the face's PBR slot registered on a material
handle the face no longer draws, so every later composition (decode, override,
maps) lands on a material nobody sees.

- **A level-of-detail rebuild built PBR faces as shared.** A render material
  given to a prim already in the scene does not re-tessellate it, and the
  inputs retained for the pixel-area driver's rebuild (`PendingPrim`,
  `PendingSculpt`, `PendingMesh`) kept the material-intern context of the last
  real build, whose `pbr_faces` was empty. The next rebuild (in the stage, the
  camera returning behind the avatar after build mode) interned the faces onto
  the shared plywood material, `register_pbr_materials` registered and
  composed the glTF material onto that shared handle — turning every identical
  plywood face PBR — and the detach net then gave the face a private plywood
  copy no slot knew about. A block refresh that does not rebuild now brings the
  retained inputs' intern context up to the update just applied
  (`ObjectBuilds::refresh_intern`).
- **A render material arriving on a still-shared face was composed onto the
  shared handle.** `update_objects` runs before
  `register_changed_render_materials` in the same frame, so a material given
  to an unselected prim registered the shared handle; same ending. Both
  registration systems now skip a face marked `SharedFaceMaterial`, and
  `detach_face` writes the face's `PrimFaceEntity` again with its private
  material, so `register_pbr_materials` registers it on the handle it draws.

The stage's box was selected (so already private) when it got its material,
which fits the first race: three runs in four failing then, none in four now, is
consistent with the rebuild's timing having moved, not proof.

Two `world_test` tests cover it, each failing without its half of the fix: a
render material given to one of two identical prims leaves the other Blinn-Phong
and turns the first PBR, and a pixel-area rebuild after a late render material
never puts a PBR face on a shared material, on any frame, and leaves it PBR. A
fifth aditi run of the stage with the fix drew the metal. The `registered` /
`assigned` debug lines now name the face entity and the handle, so a recurrence
says which handle the slot holds.
