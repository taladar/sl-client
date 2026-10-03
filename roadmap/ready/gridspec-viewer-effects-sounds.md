---
id: gridspec-viewer-effects-sounds
title: Viewer effects and sound relays on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-viewer-effect-render]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim relays `ViewerEffect`; the fake grid relays neither effects nor client
`SoundTrigger`.

## Discover

Two avatars: B emits a beam / gesture sound, A records relay range, filtering,
owner and position; `PreloadSound` on arrival.

## Document

`book/src/gridspec/avatars.md` § Effects and sounds.

## Fake grid

Small — relays and timeline actions in this task.

## Viewer

Effects rendered, sounds from both paths.
