---
id: viewer-p33-2
title: Per-object local reflection probes
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 33 — Reflection probes
---

Context: [context/viewer.md](../context/viewer.md).

**P33.2. Per-object local reflection probes.** Place the ingested
`ObjectReflectionProbe`s as Bevy `LightProbe` + `EnvironmentMapLight` box /
sphere volumes (from the prim scale + the box-volume flag), each with its own
captured cubemap, selected by a nearest-N budget the way local lights (P25)
are. Reuses the default probe's capture machinery. Deferred out of P33.1 so
the global fallback ships first.
