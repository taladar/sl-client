---
id: viewer-environment-personal-lighting
title: Personal lighting — local environment override
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-phototools, viewer-environment-fixed-editor]
---

Context: [context/viewer.md](../context/viewer.md).

The "Personal Lighting" floater: override the environment **locally** —
region settings untouched, nothing published — with immediate sliders for
sun/moon position, sun colour, ambient, haze, cloud coverage and the other
high-traffic sky knobs, plus a water section; a reset returns to the region
environment. This is the local-override layer the P22 environment pipeline
needs anyway (a settings source that shadows the region's EEP values), and
[[viewer-phototools]] explicitly builds its environment half on it.

Scope: the override layer in `environment.rs` (region ⊂ parcel ⊂ local
precedence, matching EEP semantics), the floater with live-updating
sliders, apply-a-preset (built-in Linden day frames already ported in
`render_scene.rs`; inventory settings assets arrive with
[[viewer-environment-fixed-editor]]), and reset.

Reference (Firestorm, read-only): `llfloaterenvironmentadjust`,
`floater_adjust_environment.xml`, `llenvironment` (ENV_LOCAL layer).

Builds on: the P22 EEP ingest + sky/water renderers.
