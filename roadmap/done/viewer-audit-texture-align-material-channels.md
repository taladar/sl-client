---
id: viewer-audit-texture-align-material-channels
title: Align planar faces does not propagate to the normal and specular transforms
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 3
---

Done (2026-09-13). All three audited divergences are fixed, and pinning the
arithmetic against a Firestorm extract turned up a fourth the audit had not
seen: the ported quaternion composition ran in the wrong order, so **every**
align produced the wrong rotation.

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-edit/src/edit_texture_align.rs:129-137` wrote only `dst.scale_s` /
`scale_t` / `offset_s` / `offset_t` / `rotation` — the diffuse channel. The
reference applies the same aligned values to all three:
`fspanelface.cpp:1442-1449` calls `setNormalRotation`, `setSpecularRotation`,
`setNormalOffsetX/Y` and `setNormalRepeatX/Y`.

The codebase already models these channels
(`sl-viewer-world-objects/src/legacy_materials.rs:489-496`,
`sl-viewer-edit/src/edit_material.rs:211-220`), so a bump-mapped face kept an
unaligned bump after Align.

Two further divergences in the same file were listed as lower confidence and
worth confirming against the reference first. Both were confirmed and both are
fixed: `:93` picked the lowest-indexed selected face as the anchor where the
reference uses `LLSelectedTE::getFace(last_face, …)` (`fspanelface.cpp:1609`),
and `:62-80` bailed on anything but a single primary `PRIMITIVE`, where the
reference runs `applyToTEs` across the whole selection — its Align Textures
button is in fact only enabled when `getObjectCount() > 1` (`:1928`), so
multi-object planar align was missing entirely.

## The fourth divergence: the composition order was reversed

`LLQuaternion`'s `operator*` is the standard quaternion product **with its
arguments swapped** (`llquaternion.cpp:534`) — `a * b` there rotates by `a` and
then by `b`, which is `b * a` in `glam`. The port had kept the reference's
textual order:

```rust
    let orig_st_rot = Quat::from_axis_angle(Vec3::Z, ref_te.rotation) * ref_proj.face_rot;
    let this_st_rot = orig_st_rot * proj.face_rot.conjugate();
```

so it composed the reverse of what `calcAlignedPlanarTE` composes. The two
agree only when the factors commute, which they do not: aligning a plain cube's
`+X` face to its `+Y` face — the simplest case there is — gave a texture
rotation of `-π/2` where the reference gives `0`.

`LLQuaternion(x_axis, y_axis, z_axis)` *is* `Mat3::from_cols` despite going
through `setRows`, because `LLMatrix3::quaternion` deliberately extracts the
inverse quaternion (the `SJB:` comment in `m3math.cpp:236`); that half of the
port was right.

The euler extraction was wrong for a second, independent reason: `glam`'s
`Quat::to_euler` and the reference's `LLQuaternion::getEulerAngles` disagree
about gimbal lock, and the aligned rotation between two axis-aligned faces lands
*on* the locked case — which is where the `0` above comes from. `ll_euler_yaw`
is now a verbatim port of the reference's branch.

## The fix

`edit_texture_align.rs` is rewritten around three pure functions that the tests
pin against the reference:

- `face_projection` now carries the local frame into **Second Life world
  space** with the object's world rotation and position
  (`getPlanarProjectedParams`'s `local_rot * vol_mat.quaternion()` and
  `vol_mat.getTranslation()`), which is what makes a cross-object align
  possible; the world transform comes off the entity's `GlobalTransform` with
  the basis change stripped (`gizmos::sl_world_rotation`, `bevy_to_sl_vec`).
- `aligned_planar_te` is the rest of `calcAlignedPlanarTE`, in `glam`'s order,
  including the `centers_dist` term that was dropped as always-zero when only
  one object could be aligned.
- `ll_euler_yaw` is `LLQuaternion::getEulerAngles`' yaw, gimbal branch included.

The walk itself is now the reference's: every selected object that is a plain
prim contributes its selected faces, the anchor is the primary object's
`last_face` while it is still selected (new on `SelectedNode`, the reference's
`LLSelectNode::mLastTESelected`) and otherwise the first face of the walk, and
each touched object gets its own `ObjectImage`. Every aligned face that carries
a legacy material also gets that material re-sent with its normal and specular
offset / repeat / rotation set to the aligned diffuse values; a face with no
material stays without one, which is the net effect of the reference's
`is_need_material` branch dropping the default material it just built.

## How the numbers were pinned

A verbatim extract of `llquaternion.cpp`, `m3math.cpp` and `llface.cpp`
(`operator*`, `operator~`, `v * q`, `getEulerAngles`, `LLMatrix3::setRows` /
`::quaternion`, `planarProjection`, `getPlanarProjectedParams`,
`calcAlignedPlanarTE`) compiled with `g++ -O0` prints the expected values for
five cases: two faces of one prim with and without a rotated map, a second prim
turned about `Z` and moved, one turned about `Y` with a rotated map, and a case
with the **anchor itself** on a turned and moved prim — the one that pins the
order of the anchor's own world rotation. The generator is not committed (it
would not survive the `cpp:::clang-format` / `cpp:::doxygen` hooks); the recipe
is in the module docs.

`edit_texture_align.rs` went from 243 lines with zero tests to a module with
eleven: the projection frames, all five aligned placements, both euler branches,
the offset wrap, and the three anchor cases (the primary's last-touched face
wins, an un-picked one falls back to the first face of the walk, a whole-object
selection anchors on face `0`). `SelectionSet` gained a twelfth pinning
`last_face` itself.

## How to verify on a grid

Two planar-mapped prims, at least one bump-mapped, both selected:

- Align on two faces of one prim: the texture flows across the corner, and the
  bump flows with it rather than staying put.
- Align across the two prims: the second prim's faces continue the first's
  texture across the gap between them.
- Shift-click a different face of the primary last, then Align: the whole
  selection lines up on *that* face, not on face 0.
