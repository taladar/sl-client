---
id: viewer-animation-overrider
title: Animation Overrider (client-side AO)
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [viewer-gesture-runtime, viewer-vintage-bottom-bar]
---

Context: [context/viewer.md](../context/viewer.md).

The client-side Animation Overrider — one of Firestorm's most-used features:
replace the default locomotion animations (stand, walk, run, fly, sit,
ground-sit, jump, …) with user-chosen ones, without a scripted HUD. The
locomotion state machine is already ours (`locomotion.rs` drives the default
anims), so the AO is a per-state override table consulted where the defaults
are chosen.

Scope: AO sets (multiple named sets, one active), per-state animation lists
with cycle / randomise and cycle-time, stand-cycling, sit override on/off
separately (scripted furniture must be able to win), **import of the
Firestorm/ZHAO-II notecard config format** from a worn AO HUD's contents so
existing AOs migrate in one step, the small AO floater (set selector,
per-state pickers) and the bottom-bar quick toggle
([[viewer-vintage-bottom-bar]] reserves the slot), persistence per account.

Reference (Firestorm, read-only): `ao.cpp` / `aoengine` / `aoset`,
`floater_ao.xml`, `panel_ao.xml`.

Builds on: the locomotion state machine + animation playback, inventory
(animation items, notecard read for import).
