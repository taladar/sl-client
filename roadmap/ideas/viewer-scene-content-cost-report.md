---
id: viewer-scene-content-cost-report
title: Content cost report for the parcel and for everything in draw distance
topic: viewer
status: ideas
origin: user request (2026-10-01)
points: 8
refs: [viewer-performance-floater, viewer-avatar-complexity-limit]
---

Context: [context/viewer.md](../context/viewer.md).

Some areas of the grid render slowly, and the suspicion (user, 2026-10-01) is
a high density of **distinct, high-resolution textures**. Nothing in the
viewer shows that. This idea is a report of what the content around us costs
to render: a "parcel rendering cost", and the same numbers for everything
currently being drawn.

## Two scopes

- **The current parcel**: the objects that stand on the parcel the agent (or
  the flycam's focus) is in.
- **Everything rendered**: everything within draw distance of the own avatar,
  or of the flycam when it is flying, avatars included.

## What it reports

- **Textures, by their authored resolution**: the full size of each distinct
  texture as uploaded (its J2C header's dimensions, or what a full fetch
  would give), **not** the discard level currently drawn. A histogram of
  distinct textures by size (≤128, 256, 512, 1024, 2048 and up), and the
  total at full resolution: pixel count and the memory it would take once
  decoded. Count each texture once however many faces use it, and also say
  how many faces use it, so one shared 1024² texture reads differently from
  fifty distinct ones.
- **Mesh**: distinct mesh assets, triangles at the highest LOD and at the LOD
  drawn, and the size of the mesh assets.
- **Other content**: prims (and how many are sculpts, flexi or animesh),
  lights, particle emitters, media faces, materials (PBR and legacy) and the
  normal and specular maps they add to the texture count.
- **Avatars** (in the draw-distance scope): count, the complexity model's
  score per avatar ([[viewer-avatar-complexity-limit]]), and their textures
  and meshes counted into the same histograms, or shown separately.

## Open questions before promoting

- Where it lives: a tab of the performance floater
  ([[viewer-performance-floater]]), or a window of its own with a choice of
  scope.
- How to learn a texture's authored resolution without fetching every
  texture in full. The codestream header from the first range request may
  already carry the image size.
- Whether the report should list the worst offenders (objects or linksets by
  distinct texture memory) so a builder can find them, the way the
  reference's scene-load statistics and texture console do.
