---
id: viewer-audit-gpu-pick-slot-lifecycle
title: Pick slots are recycled with no generation, and a lost try_insert leaks one
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

Two entity-lifecycle defects in `sl-viewer-world-view/src/gpu_pick.rs`, both in
the path of every click:

- `:275` — `self.object_free.pop()` reuses the raw slot index and
  `encode_pick_tag(CLASS_OBJECT_FACE, index)` carries **no generation bits**,
  while readback resolves the captured tag 2-3 frames later (`:1007`). A face
  re-tessellated across a despawn/respawn that popped the freed slot answers the
  click meant for the old object. Aggravated by `assign_*` and `free_pick_tags`
  sharing one unordered tuple (`:1112`).
- `:289` — `entity_tags.insert(entity, tag)` is immediate while
  `commands.entity(entity).try_insert((PickId(tag), MeshTag(tag)))` is deferred,
  and the code's own comment names the case where the insert is lost. No
  `PickId` means no `RemovedComponents`, so neither `entity_tags` nor
  `object_slots` is ever freed. (The general free path via
  `RemovedComponents<PickId>` is correct — this is the hole in it.)

Fix: pack a generation into the tag and compare it on readback; and record the
slot only once the component is known to exist. This is the project's own
entity-keyed-map rule applied to a map that legitimately outlives the despawn.
