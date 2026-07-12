---
id: missing-batch-4
title: scene & appearance
topic: missing
status: done
origin: MISSING_ROADMAP.md
---

Context: [context/missing.md](../context/missing.md).

## Batch 4 — scene & appearance

`ObjectAnimation` (High 30): per-object animation start/stop (animesh).
`RebakeAvatarTextures` (Low 87): server request to rebake and re-upload
appearance.

Implemented as `Event::ObjectAnimation { object_id: ObjectKey, animations:
Vec<ObjectPlayingAnimation> }` (the object analogue of `Event::AvatarAnimation`;
the simulator sends the full authoritative set of animations signalled on an
animesh object's control avatar, not a delta). `ObjectPlayingAnimation {
anim_id: AnimationKey, sequence_id: i32 }` lives in `types/object.rs`; it omits
the `source_id` of `PlayingAnimation` because an animesh object is its own
animation source. `RebakeAvatarTextures` surfaces as
`Event::RebakeAvatarTextures {
texture_id: TextureKey }` — the baked texture the simulator could not find and
wants re-uploaded.
