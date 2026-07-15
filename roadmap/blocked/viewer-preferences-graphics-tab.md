---
id: viewer-preferences-graphics-tab
title: Preferences — graphics tab
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
---

Context: [context/viewer.md](../context/viewer.md).

The **graphics** tab of the preferences floater
([[viewer-preferences-floater]]): draw distance, render-quality tiers, shadows,
reflection probes, the tone mapper / exposure, ambient occlusion, anti-aliasing,
avatar-complexity / imposter limits, and the other render knobs — each control
bound to the typed settings store through the floater's binding.

Render effects ship enabled-by-default and env-gated; this tab surfaces the
settings that drive them, it is not a build prerequisite for any effect.

Reference (Firestorm, read-only): `llfloaterpreference*` (the graphics panels).

Builds on: [[viewer-preferences-floater]].
