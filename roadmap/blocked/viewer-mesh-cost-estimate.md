---
id: viewer-mesh-cost-estimate
title: Streaming cost / land-impact estimate
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
blocked_by: [viewer-mesh-encoder]
---

Context: [context/viewer.md](../context/viewer.md).

Compute the **streaming cost / land impact** of an encoded model client-side,
from the viewer's own formula — no server round-trip needed for the estimate. It
is fully reproducible from the encoded section sizes ([[viewer-mesh-encoder]]):

- estimated triangles per LOD from each LOD section's byte size
  (`MeshBytesPerTriangle = 16`, `MeshMetaDataDiscount = 384`, with a floor);
- radius-weighted over the LOD-switch annuli (`radius/0.03`, `/0.06`, `/0.24`;
  512 m view cap; ~102944 m² region area);
- times `15000 / MeshTriangleBudget`.

**Match the viewer's `LLMeshCostData`** (in `secondlife/viewer`, not the wiki)
for the exact numbers.

**Scope note — this is the estimate only.** The L$ upload *fee* is
**server-side**; the viewer only displays it. A fee shown before upload comes
from the step-1 fee round-trip in [[viewer-mesh-upload-sequence]], not from this
formula. This task owns the land-impact figure the preview shows and the
sanity-check against what the server later returns.

Reference (Firestorm, read-only): `llmeshrepository.cpp` (`LLMeshCostData`).

Builds on: [[viewer-mesh-encoder]] (the encoded section sizes are its input).
