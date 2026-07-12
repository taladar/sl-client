---
id: viewer-p8-2
title: Stitch modes
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 8 — `sl-sculpt` (sculpt-texture → geometry)
---

Context: [context/viewer.md](../context/viewer.md).

**P8.2. Stitch modes.** Stitch per type — plane (no wrap), cylinder
(wrap U), sphere (wrap U + collapse the pole rows), torus (wrap U + V); honour
the mirror / invert flags (winding / normals). Build indices, per-vertex
normals, and grid UVs; emit a single `PrimFace`. Fall back to a placeholder
grid on a degenerate map (never panic). Seam / pole vertices are *shared* (one
canonical vertex per lattice slot, wrapped edges fold to column / row `0`,
pole rows collapse to a single vertex), so accumulated normals are smooth
across them with no seam-wrapping pass. The flags follow Firestorm's
`sculptGenerateMapVertices` — `reverse_u = invert XOR mirror` reverses the U
sampling and `mirror` negates X — which, with one fixed triangle winding,
compose to the four intended facings (so no separate winding flip). The
degenerate fallback is a procedural sphere placeholder.
