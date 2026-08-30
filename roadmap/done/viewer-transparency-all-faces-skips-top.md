---
id: viewer-transparency-all-faces-skips-top
title: Changing transparency with no face selected leaves a cube's top face alone
topic: viewer
status: done
origin: user report while verifying viewer-underwater-name-tags-not-drawn (2026-08-29)
refs: [viewer-edit-action-disabled-though-editable]
---

Context: [context/viewer.md](../context/viewer.md).

Editing **Transparency** on a cube with **no individual face selected** — which
means "apply to every face" — was reported to change the other faces but leave
the **top** one unchanged.

**Outcome (2026-08-30): the *edit* does not skip the top face — the whole chain
is correct at every face, now pinned by tests.** What remains is how that face
is **drawn**, which is a different question and is split out as
[[viewer-translucent-top-face-reads-opaque]]. A live run against the local
OpenSim (rez a box, select it whole, Transparency = 50, Enter) turned *all six*
faces translucent, and the per-face diagnostics show the edit intact at each of
the three places it could have come apart:

- the commit found **6 rendered faces** and expanded "every face" to
  `[0, 1, 2, 3, 4, 5]` — face 0, the top cap, included;
- the entry it put on the wire carried `rgba[255, 255, 255, 128]` on **all six**
  faces;
- the sim's echo re-tessellated the prim and each of the six faces was rebuilt
  from `rgba[255, 255, 255, 128]`.

## What the top face is here

Face ids are the profile face enumeration index (`sl-prim/src/volume.rs:108`).
`build_square` emits `add_cap(PATH_BEGIN)` first, then the four sides, and
`finish()` appends `add_cap(PATH_END)` (`sl-prim/src/profile.rs:487`, `669`), so
a box is **face 0 = top cap** (+Z), faces 1–4 = sides, face 5 = bottom cap (−Z),
and all six tessellate non-empty — pinned by
`box_face_ids_run_top_cap_sides_bottom_cap` (`sl-prim/src/volume.rs`), since a
cap that tessellated *empty* would have no face entity, hence no slot in the
entry a commit rebuilds from the rendered faces.

## The guards left behind

- `sl-prim`: `box_face_ids_run_top_cap_sides_bottom_cap` — a box's six faces are
  ids `0..=5`, none empty, face 0 facing +Z and face 5 −Z.
- `sl-viewer-edit`:
  `a_whole_object_edit_reaches_every_face_including_the_top_cap` — the commit
  path over a box's six rendered face entities (`PrimFaceLookup::current_faces`
  → `node_face_indices` → `apply_edit_to_faces`) writes the new alpha to every
  face, and it survives `encode_texture_entry` / `decode_texture_entry` (the
  run-length packing writes the **last** face as the field default, so a face
  dropped from the packing would decode back opaque).
- The `sl_viewer::texture_edit` tracing target (`TEXTURE_EDIT_LOG_TARGET`,
  `sl-viewer-world-objects/src/objects.rs`): with
  `RUST_LOG=info,sl_viewer::texture_edit=debug` one edit prints the faces the
  commit found, the entry it sent, and the `TextureFace` every face is rebuilt
  from after the echo — the three-point read that closed this report, kept for
  the next one.

## Not covered by the reproduction

The run exercised a single unlinked prim with a plain diffuse on every face. Not
retried: a **linkset** (each part rebuilds its own entry), a **per-face**
selection, and a face carrying a **PBR render material** — a PBR face's material
is rewritten by the P27.1 pipeline and does not take the Blinn-Phong tint at
all, so an edit there is *expected* to leave it looking unchanged. If the report
recurs, note which of those applies before reopening.
