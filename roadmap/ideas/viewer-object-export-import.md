---
id: viewer-object-export-import
title: Object backup export / import (glTF-based)
topic: viewer
status: ideas
origin: Vintage-parity coverage audit (2026-07-22)
refs: [viewer-mesh-gltf-import, viewer-mesh-encoder]
---

Context: [context/viewer.md](../context/viewer.md).

Back up own creations to disk and restore them: export a selected linkset
(geometry, textures, materials, prim parameters, task inventory where
permitted) and re-import/rez it later. **glTF-based** (user decision,
2026-07-22): not Collada — dropped from Blender and effectively dead —
so the export target is glTF 2.0 plus a sidecar for the SL-specific data
glTF cannot carry (prim parameters, task inventory), and import reuses
the [[viewer-mesh-gltf-import]] / [[viewer-mesh-encoder]] upload path.

**Strict permission gating** is the load-bearing requirement, per the
reference implementations: export only content where the agent is the
*creator* (FS's rule) — of every prim *and* every texture — with the
same checks on import-by-reference; document the policy in-UI.

Idea-stage questions: texture export rights UX (creator-only textures
silently substituted vs blocked), and whether OpenSim's laxer OAR/IAR
world should get a grid-gated wider mode.

Reference (Firestorm, read-only): `fsexport` / `floater_fs_export.xml`
(the permission model; its OXP format is not the target).
