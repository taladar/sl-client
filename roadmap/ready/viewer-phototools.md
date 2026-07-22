---
id: viewer-phototools
title: Phototools — a photographer's environment & graphics control panel
topic: viewer
status: ready
origin: user request (2026-07)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-preferences-floater, viewer-quick-preferences, viewer-snapshot-floater, viewer-camera-third-person-orbit, viewer-depth-of-field, viewer-glow-bloom, viewer-screen-space-reflections, viewer-realtime-mirrors, viewer-projector-lights-textured, viewer-pbr-terrain, viewer-antialiasing-post, viewer-avatar-impostors-billboard, viewer-ambient-occlusion, viewer-tonemap-auto-exposure]
---

Context: [context/viewer.md](../context/viewer.md).

A single control panel that gathers everything an SL photographer tweaks to get
a shot — **environment** (time of day, sky / water look) and **graphics
quality** (shadows, depth of field, exposure, ambient occlusion, lighting) — so
they can dial in the image without diving through Preferences or the environment
editor between every frame. Firestorm's Phototools floater is exactly this, and
it is telling that it is the **largest single XUI layout in the whole viewer**
(~5000 lines): photographers live in it. It is a floater
([[viewer-ui-widget-scaffold]]).

Two halves:

- **Personal environment override.** Force a local time of day / sky / water —
  midday, sunset, a custom preset — regardless of what the region sends, and
  scrub the sun freely. The environment *rendering* already exists (`sky.rs`,
  the P22 day-cycle and EEP ingest); the **local override** layer is now its
  own task, [[viewer-environment-personal-lighting]] — this floater's
  environment half is quick access to it (full EEP asset authoring is
  [[viewer-environment-fixed-editor]] and siblings). The reference's exodus
  **vignette** post effect belongs in the graphics half here.
- **Graphics quick-toggles.** The render knobs that change the *look*, surfaced
  live: shadows (P24), the reflection probes (P33), exposure / the tone mapper
  (P33.3), point-light limits, draw distance, and the render-quality tiers —
  each bound to the same settings store everything else uses, so a change here
  is the same change Preferences would make. Several of the knobs Phototools
  exposes are render features **we have not built yet** — this floater is
  effectively Firestorm's own catalogue of them, and the gap analysis (2026-07)
  turned each into its own task: [[viewer-depth-of-field]],
  [[viewer-glow-bloom]], [[viewer-screen-space-reflections]],
  [[viewer-realtime-mirrors]], [[viewer-projector-lights-textured]],
  [[viewer-pbr-terrain]], [[viewer-antialiasing-post]],
  [[viewer-avatar-impostors-billboard]], [[viewer-ambient-occlusion]] and
  [[viewer-tonemap-auto-exposure]]. Phototools *surfaces* them; it does not
  block on them (it exposes whatever exists).

This is deliberately a
**sibling of [[viewer-quick-preferences]], not a duplicate**: quick-prefs is the
general "settings I reach for often" panel; Phototools is the
*photography preset* of that idea, plus the environment override, plus a layout
tuned for composing a shot. Build it as a curated view over the typed settings
store ([[viewer-preferences-floater]]) rather than a parallel pile of controls,
so the two share plumbing. It pairs naturally with the snapshot floater
([[viewer-snapshot-floater]]) — set the look here, capture there — and with
[[viewer-camera-third-person-orbit]] for the framing.

Reference (Firestorm, read-only): `floater_phototools.xml` (the layout),
`fsfloaterphototools`, and the environment / EEP panels
(`llfloatereditextdaycycle`, `llpanelenvironment`).
