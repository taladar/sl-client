---
id: viewer-mouselook-combat
title: Mouselook combat conveniences
topic: viewer
status: ideas
origin: debug-settings/chat-lines survey (2026-07-23)
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's mouselook combat cluster, for combat-sim players: extra
mouselook behaviours (`FSMouselookCombatFeatures`) such as instant
weapon raycasts staying aligned, plus scroll-wheel handling —
`FSScrollWheelExitsMouselook` (wheel-back leaves mouselook) and
`FSDisableMouseWheelCameraZoom`. Needs scoping against our mouselook
camera before it can be a ready task: enumerate what
`FSMouselookCombatFeatures` actually gates in the FS source and which
parts apply to our input pipeline.

Reference (Firestorm, read-only): the `FSMouselook*` /
`FSScrollWheelExitsMouselook` settings and their consumers in
`llagentcamera.cpp` / FS patches.
