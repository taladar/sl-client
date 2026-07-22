---
id: viewer-rlv-vision-render
title: RLV vision-restriction rendering
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-rlv-restriction-state]
---

Context: [context/viewer.md](../context/viewer.md).

The RLVa **vision** restrictions — the render-side effects the restriction
state machine ([[viewer-rlv-restriction-state]]) can demand:
`@setsphere` (the RLVa 2.9+ sphere system: blur / blend / darken /
chromatic distortion applied outside/inside a sphere around the avatar,
with distance ramps), and the older `@camdrawmin/max` fog-out limits.
Implemented as a post-process node parameterised from the restriction
state (our underwater-fog post effect is the pattern to copy), honouring
the RLVa semantics for combining multiple issuers (most restrictive
wins per parameter).

Reference (Firestorm, read-only): `rlveffects` (`RlvSphereEffect`),
`llvfx` / `rlvF.glsl` shaders.

Deps: [[viewer-rlv-restriction-state]] (the parameters to render from).
