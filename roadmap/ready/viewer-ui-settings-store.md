---
id: viewer-ui-settings-store
title: Typed persistent settings store
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-preferences-ui/viewer-ui-framework
---

Context: [context/viewer.md](../context/viewer.md).

A pure, typed, persistent settings store: named settings with types and sensible
defaults, load/save to disk, and per-account overrides layered over the global
defaults. No UI — this is the backend that many later tasks read and write:
input rebinding ([[viewer-input-rebinding-persistence]]), camera presets
([[viewer-camera-presets]]), i18n locale ([[viewer-i18n-locale-selection]]), the
SpaceNavigator axis mapping ([[viewer-input-spacenav-camera-mapping]]), and the
chat auto-open-on-typing toggle ([[viewer-chat-input-bar]]) all consume it.

The two-way widget binding on top of this store is a separate task
([[viewer-ui-settings-binding]]); this one owns only the store + persistence.
Model it after the reference's design-token indirection and `control_name=`
two-way binding (1,293 uses — the reason ~20 preference panels have almost no
code behind them), which is why the store is a first-class shared resource
rather than scattered per-panel state.

**Do not copy** the reference's `LLInitParam` (2,000 lines of C++ templates
reimplementing serde) — use serde.

Reference (Firestorm, read-only): `llviewercontrol` (the settings backend),
`llcontrolgroup`, `llfloatersettingsdebug` (the raw debug settings editor).
