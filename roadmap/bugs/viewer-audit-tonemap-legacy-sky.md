---
id: viewer-audit-tonemap-legacy-sky
title: ACES tonemapping is applied to legacy skies the reference exempts
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
refs: [viewer-audit-scene-change-guards-day-cycle]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/tonemap.rs:209` applies `RenderTonemapMix`
unconditionally — the file contains **zero** references to `can_auto_adjust` or
`reflection_probe_ambiance`, while the flag is already derived one module over
(`exposure.rs:790`, `can_auto_adjust: reflection_probe_ambiance == 0.0`) and
simply never consumed.

The reference exempts legacy skies entirely: `llsettingssky.cpp:2066` returns a
`0.0` tonemap mix for them, and `pipeline.cpp:7912` selects
`gNoPostTonemapProgram` when probe ambiance is 0. Both aditi and the local
OpenSim serve legacy skies, so this is wrong on **every grid currently tested
against**.

Adjacent, same function: `:213` sets `tonemap.exposure` unclamped while
`tonemap_mix` one line up is clamped.
