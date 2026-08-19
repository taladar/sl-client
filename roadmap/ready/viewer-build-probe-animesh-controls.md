---
id: viewer-build-probe-animesh-controls
title: Features tab — reflection-probe & animated-mesh controls
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-prim-parameter-editing, viewer-realtime-mirrors]
---

Context: [context/viewer.md](../context/viewer.md).

The Features tab's remaining blocks, both absent from
`sl-client-bevy-viewer/src/edit_params.rs`:

The **Reflection Probe** checkbox with its sub-controls — Volume Type
combo (Sphere / Box), Dynamic flag, Update Type combo (Static /
Dynamic / Mirror / Dynamic Mirror), and the Ambiance and Near Clip
spinners. The protocol type already exists (the `ReflectionProbe`
extra-param in `sl-proto/src/types/object.rs`) and we already *render*
probes (P33-2 done; hero mirrors via [[viewer-realtime-mirrors]] in
progress) — we just cannot author them.

The **Animated Mesh** checkbox — the animesh extra-param flag that
turns a rigged-mesh linkset into a control-avatar. We render animesh
(`sl-client-bevy-viewer/src/animesh.rs`) but cannot set the flag on an
object.

Reference (Firestorm, read-only):
`indra/newview/skins/default/xui/en/floater_tools.xml` (L2796,
3062-3155), `indra/newview/llpanelvolume.cpp`.
