---
id: viewer-prim-parameter-editing
title: Prim parameter editing
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection, viewer-ui-framework]
---

Context: [context/viewer.md](../context/viewer.md).

The object / features tabs of the edit floater: name & description,
physics / phantom / temp-on-rez flags, the prim **shape** parameters (path &
profile, cut, hollow, twist, taper, shear, dimple, revolutions), and the
light / flexi / particle feature toggles.

Reference (Firestorm, read-only): `llpanelobject`, `llpanelvolume`; messages
`ObjectShape`, `ObjectExtraParams`, `ObjectFlagUpdate`.

Builds on: `PrimShapeParams` (`sl-proto`), and the existing feature renderers
`flexi.rs`, `lights.rs`, `particles.rs`.

Deps: [[viewer-object-selection]], [[viewer-ui-framework]].
