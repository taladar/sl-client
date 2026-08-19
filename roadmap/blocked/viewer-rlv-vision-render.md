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

## Parity-audit addendum (2026-08-19)

CORRECTION: the body's `@camdrawmin` / `@camdrawmax` do not exist in
Firestorm RLVa's dictionary (verified — no `camdraw` anywhere in the
rlv* sources); they are Marine's-RLV-only commands. Do not block on
them.

Missing scope found by the audit: the `@setoverlay` overlay effect
family (`RlvOverlayEffect` in `rlveffects.cpp`) — `@setoverlay`
(screen-space textured overlay), `@setoverlay_touch` (the overlay's
alpha at the click point decides whether clicks pass through to the
world), `@setoverlay_tween=force`, and the three local modifiers alpha
/ texture / tint. The @setsphere side carries 10 local modifiers (mode,
origin, color, distmin, distmax, distextend, param, tween, valuemin,
valuemax — rlvhelper.cpp:240-251); the option-addressed local-modifier
machinery itself lives in [[viewer-rlv-restriction-state]].
