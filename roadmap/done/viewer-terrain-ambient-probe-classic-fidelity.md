---
id: viewer-terrain-ambient-probe-classic-fidelity
title: Terrain lighting — add the reference probe term + classic-mode blend
topic: viewer
status: done
origin: viewer-clouds-sun-occlusion-horizon-contact terrain rework (2026-08-03)
refs: [viewer-clouds-sun-occlusion-horizon-contact, viewer-sunlit-face-clips-two-channels]
---

Context: [context/viewer.md](../context/viewer.md).

The 2026-08-03 terrain-lighting rework moved the ground from the raw
reflection-probe irradiance (which over-cooled it to sky-blue at dawn / dusk) to
the reference legacy model: the sky frame's atmospheric ambient (`amblit`) plus
the sun's atmospheric diffuse colour (`sunlit`) × N·L, driven per frame via the
new `TerrainLighting` uniform (`terrain.wgsl` / `drive_terrain_lighting`). This
matches Firestorm's warm dawn / green noon terrain.

It is a **documented simplification** of `softenLight`'s legacy branch — two
pieces were left out:

- **The additional reflection-probe term.** The reference does
  `irradiance = amblit; sampleReflectionProbesLegacy(irradiance, …)` — the probe
  adds the surroundings' diffuse reflection *on top of* `amblit`. We dropped the
  probe entirely, so the ground no longer picks up nearby coloured surfaces / a
  bright build's bounce. Re-add it as a **small** contribution over the `amblit`
  base (not the dominant term that caused the blue), so a sun-shaded slope still
  reads warm.
- **The classic-mode gamma blend.** Classic mode (the legacy default) blends the
  sun in through an sRGB round-trip:
  `srgb_to_linear(ambient*0.9 + linear_to_srgb(min(da,scol))*sunlit*0.7)`
  with `da = pow(NdotL, 1.2)`. We use a plain linear `ambient + sunlit * NdotL`.
  Port the exact blend if a side-by-side shows a meaningful difference.

Low priority — the current result already matches Firestorm well; this is
fidelity polish.

## Done (2026-09-16), by viewer-sunlit-face-clips-two-channels

Terrain now shares the full port of `softenLight`'s legacy branch with every
other non-PBR surface ([[viewer-sunlit-face-clips-two-channels]]), classic
blend included; the `TerrainLighting` uniform is gone.

The probe half turned out to be the other way round from what this item
assumed: under a **classic** sky `sampleReflectionProbesLegacy` sets
`ambenv = amblit` and never samples the probes for the ambient, so the
reference adds no probe term to legacy terrain there. Under an **EEP** sky the
probe irradiance replaces `amblit` in proportion to the probe ambiance, which
is what the port does.
