---
id: viewer-audit-gpu-pick-slot-lifecycle
title: Pick slots are recycled with no generation, and a lost try_insert leaks one
topic: viewer
status: done
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

## Outcome (2026-09-04): a generation in the tag, and one atomic allocation

**The recycled slot.** A pick tag is now `class:4 | generation:8 | index:20`
rather than `class:4 | index:28`. Each slot table entry
(`SlotEntry<AvatarSlot>` / `SlotEntry<ObjectFaceSlot>`) carries a generation
that `free_entity` bumps every time the index goes back on the free list, and
`resolve` refuses a tag whose generation is no longer the index's. So a tag the
GPU captured over an object that has since been derendered resolves to nothing
instead of to whatever took its slot. `free_entity` checks the generation too:
a tag for an index that has already moved on frees nothing, rather than
clobbering the new occupant.

Twenty index bits is just over a million live slots per class — a dense
region's face entities run to the low hundreds of thousands and freed indices
are recycled, so the tables only ever grow to the peak *live* count. Eight
generation bits wrap every 256 reuses of one index, and a stale tag is at most
three frames old, so a false match would need that index freed and reallocated
256 times inside three frames.

**The lost `try_insert`.** Allocation and insert were two steps with a gap, and
an entity despawned in the gap never got its `PickId` — so no
`RemovedComponents<PickId>` ever fired and both its slot and its `entity_tags`
entry were held for the rest of the session. The four `assign_*` systems no
longer touch the registry at all: each collects what it wants tagged into a
`PendingPickTag` list and hands it to `queue_pick_tags`, which allocates and
inserts inside **one exclusive command**. If the entity turns out to be gone,
the allocation is unwound through the ordinary `free_entity` path, so there is
no second liveness rule to keep in step with the first.

That also settles the third complaint, the unordered tuple: nothing in `Update`
allocates any more. Allocation happens at the command flush, strictly after
every `Update` system including `free_pick_tags`, so a slot freed this frame is
back on the free list before anything allocates — whichever order the four ran
in. The systems dropped their `ResMut<PickRegistry>` and can now run in
parallel with each other, which they could not before.

Pinned by two tests, each checked against the old behaviour first (with the
generation bump and the unwind stubbed out, both fail):

- `a_recycled_slot_refuses_the_stale_tag` — a tag captured over object 7 is
  freed, its index is handed to object 99, and the captured tag resolves to
  `None` rather than to object 99.
- `a_face_despawned_before_the_insert_leaks_no_slot` — runs the real
  `assign_object_face_pick_tags`, despawns the face between the query and the
  command flush (the race, reproduced exactly), and asserts the registry holds
  no tag, no `entity_tags` entry and no allocated slot.

The two pre-existing registry tests now assert the generation as well: the
freed *index* is still reused, but the *tag* is not reissued.
