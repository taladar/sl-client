---
id: viewer-mesh-upload-sequence
title: Two-POST NewFileAgentInventory mesh upload
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-mesh-model-upload
blocked_by: [viewer-mesh-encoder]
---

Context: [context/viewer.md](../context/viewer.md).

Drive the **two-POST upload** of an encoded model to the `NewFileAgentInventory`
cap. The upload cap itself is generic (done as `protocol-23`), so nothing
mesh-specific exists on the send side yet — this task adds the mesh-upload
LLSD payloads and the two-step handshake.

- **Step 1** posts the model **without textures** and gets back
  `{state: "upload", uploader: <url>, upload_price, data: {…costs…}}`. This is
  where the L$ **fee** and the server's land-impact come from (the client-side
  estimate lives in [[viewer-mesh-cost-estimate]]), so step 1 doubles as the
  confirmation the user approves before committing.
- **Step 2** posts `asset_resources` (now with J2C textures) to the returned
  uploader URL:
  - `mesh_list[]` — the raw binary LLMesh assets ([[viewer-mesh-encoder]]);
  - `texture_list[]` — the J2C textures;
  - `instance_list[]` — per-instance transform, `physics_shape_type`, mesh
    index, and per-face material.

Reference (Firestorm, read-only): `llmeshrepository.cpp` (`LLMeshUploadThread`,
the two-step upload).

Builds on: [[viewer-mesh-encoder]] (the raw LLMesh assets) and the `protocol-23`
upload caps.
