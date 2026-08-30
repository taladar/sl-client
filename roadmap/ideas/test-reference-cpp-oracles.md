---
id: test-reference-cpp-oracles
title: Reference-viewer C++ math and GLSL as test oracles (FFI / naga)
topic: test
status: ideas
origin: user thought (2026-08-30) while planning the test harness
points: 5
refs: [viewer-render-context-matrix, viewer-render-baselines]
---

Context: [context/testing.md](../context/testing.md).

Feasible where the reference code is a deterministic function of plain
inputs, which is exactly where our ports are most likely to drift:
`LLVolume` tessellation at every LOD across path/profile/cut/twist/hollow,
`LLPolyMorph` and skin-weight tables, `LLPatch` terrain decode,
`LLSettingsSky`/water colour math, animation joint blending, `LLPrimitive`
texture-coordinate maths. Two shapes, chosen by input-space size:

- **Baked tables** (the existing pattern — a verbatim extract compiled
  once, tables committed, generator not) for small input spaces.
- **A `cc`-built `extern "C"` shim** (`sl-reference-ffi`, dev-dependency,
  feature-gated) called from tests for combinatorial spaces such as
  tessellation. The cost is stubbing `llcommon` (`llerror`, aligned
  allocation, APR/Boost pulls), not the FFI itself. Gate on
  `SL_VIEWER_REFERENCE_SRC` pointing at the Firestorm checkout and skip
  loudly otherwise; the LGPL extract stays in the third-party tree, never
  in git.

Not feasible for the render pipeline (`LLPipeline`, draw pools — OpenGL,
window, singletons). Its **GLSL shaders** are usable without FFI: compile
the reference shader through naga on a fullscreen quad with the same
uniforms and compare outputs to ours — the right oracle for sky, water and
atmospherics math, and a natural extension of the render matrix.
