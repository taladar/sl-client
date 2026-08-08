---
id: viewer-preferences-graphics-tab
title: Preferences — graphics tab
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui
blocked_by: [viewer-preferences-floater]
refs: [viewer-quick-preferences]
---

Context: [context/viewer.md](../context/viewer.md).

The **graphics** tab of the preferences floater
([[viewer-preferences-floater]]): draw distance, render-quality tiers, shadows,
reflection probes, the tone mapper / exposure, ambient occlusion, anti-aliasing,
avatar-complexity / imposter limits, and the other render knobs — each control
bound to the typed settings store through the floater's binding.

Render effects ship enabled-by-default and env-gated; this tab surfaces the
settings that drive them, it is not a build prerequisite for any effect.
Named save/load of this whole settings group is
[[viewer-graphics-presets]], which builds on this tab.

Quick Preferences: several of these are reached-for-hourly and belong in the
Quick Preferences panel too ([[viewer-quick-preferences]]) — the render-quality
tier and `RenderVolumeLODFactor` especially (draw distance is already there).
When such a setting is introduced here, add a default entry for it in
`default_entries()` (`quick_preferences.rs`) plus a Fluent label; the panel is a
view over the same store and binds by setting key, so nothing is reimplemented.

Reference (Firestorm, read-only): `llfloaterpreference*` (the graphics panels).

Builds on: [[viewer-preferences-floater]].
