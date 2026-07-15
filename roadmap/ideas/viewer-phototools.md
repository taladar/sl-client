---
id: viewer-phototools
title: Phototools — a photographer's environment & graphics control panel
topic: viewer
status: ideas
origin: user request (2026-07)
blocked_by: [viewer-ui-framework, viewer-preferences-ui]
---

Context: [context/viewer.md](../context/viewer.md).

A single control panel that gathers everything an SL photographer tweaks to get
a shot — **environment** (time of day, sky / water look) and **graphics
quality** (shadows, depth of field, exposure, ambient occlusion, lighting) — so
they can dial in the image without diving through Preferences or the environment
editor between every frame. Firestorm's Phototools floater is exactly this, and
it is telling that it is the **largest single XUI layout in the whole viewer**
(~5000 lines): photographers live in it.

Two halves:

- **Personal environment override.** Force a local time of day / sky / water —
  midday, sunset, a custom preset — regardless of what the region sends, and
  scrub the sun freely. The environment *rendering* already exists (`sky.rs`,
  the P22 day-cycle and EEP ingest); what is missing is a **local override** on
  top of the region's environment, plus quick access to it. (There is no
  personal-environment-editing task yet — this is where that photographer-facing
  slice lives; full EEP asset authoring is a larger, separate concern.)
- **Graphics quick-toggles.** The render knobs that change the *look*, surfaced
  live: shadows (P24), depth of field, exposure / the tone mapper (P33.3), the
  reflection probes (P33), ambient occlusion ([[viewer-ambient-occlusion]]),
  point-light limits, draw distance, and the render-quality tiers — each bound
  to the same settings store everything else uses, so a change here is the same
  change Preferences would make.

This is deliberately a **sibling of [[viewer-quick-preferences]], not a
duplicate**: quick-prefs is the general "settings I reach for often" panel;
Phototools is the *photography preset* of that idea, plus the environment
override, plus a layout tuned for composing a shot. Build it as a curated view
over the typed settings store rather than a parallel pile of controls, so the
two share plumbing. It pairs naturally with [[viewer-snapshot-tools]] — set the
look here, capture there — and with [[viewer-camera-system]] for the framing.

Reference (Firestorm, read-only): `floater_phototools.xml` (the layout),
`fsfloaterphototools`, and the environment / EEP panels
(`llfloatereditextdaycycle`, `llpanelenvironment`).

Deps: [[viewer-ui-framework]] (the floater), [[viewer-preferences-ui]] (the
settings store it is a view over).
