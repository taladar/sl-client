---
id: viewer-p33-3
title: Reflection-probe brightness calibration
topic: viewer
status: ready
origin: VIEWER_ROADMAP.md — Phase 33 — Reflection probes
---

Context: [context/viewer.md](../context/viewer.md).

**P33.3. Reflection-probe brightness calibration.** Calibrate the probe's
reflection / ambient contribution against the viewer's mixed material /
exposure model (custom sky / terrain / water vs `StandardMaterial`), ideally
once ambient occlusion (a reference `LLReflectionMapManager` companion) is in,
so the tuning is done against the full lighting model rather than piecemeal.
