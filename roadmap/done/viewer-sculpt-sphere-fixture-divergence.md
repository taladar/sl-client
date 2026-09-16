---
id: viewer-sculpt-sphere-fixture-divergence
title: The catalogue's sculpt sphere is a sphere here and a flat sheet in Firestorm
topic: viewer
status: done
origin: seen while settling the fixtures of viewer-pbr-face-sky-lighting-divergence (2026-09-16)
refs:
  - viewer-pbr-face-sky-lighting-divergence
  - viewer-p9-1
  - viewer-texture-anisotropic-filtering-missing
---

Context: [context/viewer.md](../context/viewer.md).

In every `sl-crosscheck --scenario catalogue --look-at pbr-box --look-from 3
--look-above 4` run (the pose puts `sculpt-sphere`, at `128,136,25.5`, at the
left of the frame), this viewer draws a sphere and Firestorm draws a **thin
flat sheet**, tilted, wearing the checker. It is the same in every frame of a
30-frame run, so it is not a sculpt map still loading.

What the two scene dumps say about the object: both call it `is_sculpt`, both
at LOD 3, same position and scale — but `num_faces` is **1** here and **6** in
Firestorm, which is a box's count. (The field's meaning differs by design —
drawn faces here, `getNumTEs` there — so on its own that is a hint, not proof,
that the reference never built the sculpt surface.)

Neither log says anything about the sculpt map (`SCULPT_MAP`,
`00000000-0000-0000-0000-00000ca70002`, `sl_test_assets::sculpt_sphere(64)`).

Our sphere is not obviously right either: its checker has thin stray bands
across it that a UV sphere should not show.

## To find out

Which side is wrong, and whether it is the fixture again (as the PBR box's
material asset was): the sculpt map's encoding (a 64² RGB JPEG2000 — does the
reference's OpenJPEG decode it to the raw image `LLVOVolume::sculpt` needs, at
the discard level it asks for?), the sculpt type and stitching bits in the
extra params, and `LLVolume::sculpt`'s placeholder path when the data is
missing.

## Answer

**Both sides were wrong, in different ways.** The fixture gave the sculpt a
box's shape, and this viewer ignored the shape of every sculpt.

The reference does not treat a sculpt as a shape of its own. `LLVolume::sculpt`
generates the prim's **own path and profile** — asking only a *circle* path and
a *circle* profile for as many steps as the map is worth — and then reads each
vertex position out of the map instead of sweeping the profile. So the shape
decides the grid's depth and width, the faces (and which are caps) and every
texture coordinate; the map decides only where the vertices are.

`PrimFixture::sculpt` set the sculpt block but left `box_prim`'s square
profile on a line path. On that shape:

1. the line path is **two** frames deep whatever the map, and both rows pinch
   to a pole, so every vertex sits on one of two points;
2. the surface has no area, fails `sculptGetSurfaceArea`'s test, and becomes
   `sculptGenerateSpherePlaceholder` — whose two rows, azimuth 0 and a full
   turn, are the *same* half circle;
3. the box's four sides are zero-area strips between those identical rows,
   and its two **caps** are the half-circle polygon: a flat half-disc in the
   prim's XZ plane, wearing the checker.

That is the tilted sheet, and `getNumTEs` = 6 was the box's faces all along.

`sl-sculpt` meanwhile resampled the map onto its own grid, bilinearly, and
never looked at the shape — so it drew a sphere on a box. It also **shared** its
seam and pole vertices, and a shared seam vertex cannot carry both texture U 0
and U 1: the last column's U ran from 31/32 back to 0, squeezing a reversed copy
of the whole texture into one column. Those were the stray bands.

## Fix

- **`sl-prim`**: `Path::generate_sculpted` / `Profile::generate_sculpted` (the
  `is_sculpted` sizing, no split) and `tessellate_sculpted`, which assembles a
  sculpt's faces from a supplied surface grid through the same side / cap
  builders a prim uses, with `createSide`'s sculpt normal branch (pole
  averaging, S and T seam wraps) and the invert-XOR-mirror U reversal. The
  side normals also gained the reference's "even out quad contributions"
  extra weight, which the prim port had dropped — prims shade by it too.
- **`sl-sculpt`**: `tessellate(map, sculpt_type, shape, lod)` is now a port of
  `LLVolume::sculpt` + `sculptGenerateMapVertices`: nearest texel below each
  vertex's fraction, sphere pinch, wrap per type, mirror / invert, the area
  test with its sphere placeholder (skipped at the lowest level, as the
  reference does for legacy content), and the empty placeholder for a map
  without positions (no pixels, or fewer than three components).
- **viewer**: the shape rides `PendingSculpt` and the `GeometryKey::Sculpt`
  cache key (the same map on another shape is other faces).
- **fixture**: `PrimFixture::sculpt` sets `sculpt_shape()` — circle on circle,
  top size 1.0 × 0.5, what OpenSim's `PRIM_TYPE_SCULPT` sets and the build
  tool's sculpt type matches.
- The gallery's `sculpt-sphere` scene is laid over the same shape; its baseline
  counts are now `(steps + 1)²` vertices (33², 17², 9², 7²), and it no longer
  declares Z symmetry — nearest sampling reads rows 0, 2, … 62, 63 of a 64-row
  map, which does not mirror about the equator. The reference draws that too.

## Verified

`sl-crosscheck --scenario catalogue --look-at pbr-box --look-from 3
--look-above 4`: both viewers draw the sculpt as a sphere with the same
silhouette and the same checker layout, one face each in both dumps.

One visible difference remains on the sphere and is not this: our checker
edges are softer where Firestorm's are crisp, while the boxes in the same frame
are equally sharp in both. The sphere's V spans pole to pole, about twice the
texel density of U; the reference filters anisotropically by default
(`RenderAnisotropic`) and this viewer sets no sampler anisotropy anywhere.
Filed as [[viewer-texture-anisotropic-filtering-missing]].
