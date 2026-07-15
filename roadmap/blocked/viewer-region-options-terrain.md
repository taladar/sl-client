---
id: viewer-region-options-terrain
title: Region / Estate floater — terrain tab
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-region-options-debug]
---

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater **terrain** tab: the four terrain detail textures
and their elevation ranges (low / high per corner), water height, terrain raise
/ lower limits. Adds a tab to the floater shell from
[[viewer-region-options-debug]]; the terrain-texture and elevation edits overlap
with the terrain-editing brush work.

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`
(terrain panel).

Builds on: `protocol-14` estate / region.

Deps: [[viewer-region-options-debug]].
