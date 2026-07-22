---
id: viewer-inventory-sound-anim-preview
title: Inventory sound & animation preview
topic: viewer
status: blocked
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-audio-backend]
refs: [viewer-inventory-open-and-properties]
---

Context: [context/viewer.md](../context/viewer.md).

The item-preview players the per-type Open path
([[viewer-inventory-open-and-properties]]) still lacks:

- **Sound preview**: open a sound item → a small floater with play
  (locally, through [[viewer-audio-backend]]'s UI bus) / play in-world
  (`SoundTrigger` send — protocol present), gain slider, duration.
- **Animation preview**: open an animation item → play on own avatar
  (locally only vs broadcast — both wire paths exist) with start/stop,
  and the asset's priority/duration/loop metadata displayed from the
  `sl-anim` decode.

Small, but the everyday way inventories get sorted ("which of these five
identically-named sounds is the doorbell").

Reference (Firestorm, read-only): `llpreviewsound`, `llpreviewanim`.

Deps: [[viewer-audio-backend]] (local sound playback; the animation half
alone would not justify splitting the task).
