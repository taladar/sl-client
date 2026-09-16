---
id: viewer-sculpt-sphere-fixture-divergence
title: The catalogue's sculpt sphere is a sphere here and a flat sheet in Firestorm
topic: viewer
status: bugs
origin: seen while settling the fixtures of viewer-pbr-face-sky-lighting-divergence (2026-09-16)
refs: [viewer-pbr-face-sky-lighting-divergence, viewer-p9-1]
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
