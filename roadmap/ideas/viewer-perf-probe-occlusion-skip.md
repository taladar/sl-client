---
id: viewer-perf-probe-occlusion-skip
title: Skip capture for occluded reflection probes
topic: viewer
status: ideas
origin: reflection-probe performance planning round (2026-07-22), Firestorm LLReflectionMapManager survey
refs: [viewer-perf-probe-instrumentation]
blocked_by: [viewer-perf-probe-scheduling]
---

Context: [context/viewer.md](../context/viewer.md).

The reference issues GPU occlusion queries (`GL_ANY_SAMPLES_PASSED`
against each probe's bounding cube) and deprioritizes probes nobody can
see — an occluded probe gets only a cheap origin refresh. Ported here
this would land as a **scheduler score input** (hence blocked on
[[viewer-perf-probe-scheduling]]): an occluded dirty probe waits until
it is visible again.

Speculative because Bevy 0.19's experimental two-phase GPU occlusion
culling is mesh-granular and exposes no per-volume query API — it would
need either reading back the culling phase's visibility verdict for a
proxy mesh on the probe bounds, or a custom occlusion-query render node.
Under the change-driven scheduler the payoff is also smaller than in the
reference (an occluded *clean* probe already costs nothing); it only
helps scenes where occluded probes are frequently dirtied (busy
interiors behind walls). Measure that with
[[viewer-perf-probe-instrumentation]] counters before building anything.

Verification sketch: capture counters show no bursts for a dirtied probe
behind a wall, and prompt resumption when the camera rounds the corner.
