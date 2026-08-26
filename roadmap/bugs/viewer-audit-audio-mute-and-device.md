---
id: viewer-audit-audio-mute-and-device
title: Collision sounds ignore the object-sound mute exception, and a device fallback lies to the UI
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

- `sl-viewer-audio/src/world_sounds.rs:575` — `ingest_collisions` checks
  `mutes.is_muted(key.uuid())`, while the trigger and attached-sound paths
  (`:479`) go through `muted()` ->
  `is_muted_aspect(.., MuteFlags::ALLOW_OBJECT_SOUNDS)`. So a mute entry that
  **allows** object sounds still silences that object's collision sounds. Both
  paths should share one tested predicate.
- `sl-viewer-audio/src/audio.rs:238` — on a failed named-device open the mixer
  falls back to Default but records `*last = Some(stored)` (the *requested*
  name) and never corrects the setting, so the preferences combo keeps showing a
  device that is not in use, with only a `warn!`. It is never retried if the
  device reappears.

Two documentation contradictions in the same file worth resolving while there:
`world_sounds.rs:495` says "anything unrecognised map to plastic" while the call
site at `:584` says "unknown -> wood". Behaviour: an unknown material *byte*
gives plastic, a missing lookup gives `unwrap_or(3)` = wood. And
`collision_sound_str` (`:501`, `:587`) returns UUID **strings** that
`Uuid::parse_str` re-parses on every collision edge — that should be a const
table.

`audible_from_flags` (`:124`) is already pure and has no test for its
three-boolean truth table.
