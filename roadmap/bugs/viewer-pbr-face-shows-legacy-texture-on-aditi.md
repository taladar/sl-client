---
id: viewer-pbr-face-shows-legacy-texture-on-aditi
title: A prim given a PBR material from the Build window sometimes keeps drawing its plywood
topic: viewer
status: bugs
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
