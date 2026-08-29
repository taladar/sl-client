---
id: viewer-transparency-all-faces-skips-top
title: Changing transparency with no face selected leaves a cube's top face alone
topic: viewer
status: bugs
origin: user report while verifying viewer-underwater-name-tags-not-drawn (2026-08-29)
refs: [viewer-edit-action-disabled-though-editable]
---

Context: [context/viewer.md](../context/viewer.md).

Editing **Transparency** on a cube with **no individual face selected** — which
means "apply to every face" — changes the other faces but leaves the **top**
one unchanged.

## What the top face is here

Face ids are the profile face enumeration index (`sl-prim/src/volume.rs:108`).
`build_square` emits `add_cap(PATH_BEGIN)` first, then the four sides, and
`finish()` appends `add_cap(PATH_END)` (`sl-prim/src/profile.rs:487`, `669`), so
a box is **face 0 = top cap**, faces 1–4 = sides, face 5 = bottom cap.

That matters because face 0 is the *first* index, so a plain off-by-one at the
end of a range cannot explain it — the top is the one index a `0..n` loop can
never miss.

## What was checked and looks correct

- "All faces" is `SelectedNode.faces: None`
  (`sl-viewer-world-api/src/lib.rs:84`), expanded by `node_face_indices`
  (`edit_texture.rs:1827`) to `(0..face_count)` — index 0 included.
- `apply_edit_to_faces` (`edit_texture.rs:1843`) writes by
  `entry.faces.get_mut(index)`.
- `TexField::Transparency` (`edit_texture.rs:211`) writes byte 3 of
  `face.color`.
- The wire codec round-trip: `pack_field` / `unpack_field`
  (`sl-proto/src/appearance.rs:147`, `333`) reach index 0 in both directions,
  with no face-0/face-max asymmetry.

## Candidates, most likely first

1. **The change reaches the wire but not the screen.** On a default cube all six
   faces intern to a *single* shared material
   (`material_cache.rs:245`), and the copy-on-write detach
   `detach_shared_face_materials` (`material_cache.rs:296`) is what gives an
   edited face its own. A face that fails to detach keeps rendering the old
   alpha even though correct data went out. This fits a *single* odd face out.
2. **The face count is the max rendered face id + 1, not the prim's real face
   count.** `PrimFaceLookup::current_faces` (`edit_texture.rs:1752`) builds the
   entry from the *rendered* face entities: `count = max(face_id) + 1`, and
   `spawn_prim_faces` skips any face where `face.is_empty()`
   (`sl-viewer-world-objects/src/objects.rs:2412`), so a face with no geometry
   has no entity. A missing id 0 would leave slot 0 as an all-neutral
   `TextureFace` with the edit applied on top — the top face would come back
   nil-textured/white rather than merely unchanged, so this predicts a *visibly
   different* wrong result than reported. Worth confirming which is seen.
3. **The displayed value is face 0's only.** `representative_face`
   (`edit_texture.rs:1076`) decodes the blob against
   `min(selected face) + 1` slots — with nothing selected that is **one** slot,
   i.e. face 0, the top. So the Transparency box shows the top face's value
   whatever the other five are. That alone can make a correct edit *look* like
   the top did not change: the box keeps reading the old value. This may be the
   whole report rather than a rendering fault at all.

## Decisive first test

Apply the change, then force a re-read — reselect the object, or relog. If the
top face then shows the new transparency, the write path is fine and this is
candidate 1 or 3 (display / material detach). If it is still unchanged, the
fault is on the write side, and candidate 2 is next: log `faces.len()` and
`node_face_indices(...)` at `edit_texture.rs:1810` for a default cube and check
they are `6` and `[0,1,2,3,4,5]`.
