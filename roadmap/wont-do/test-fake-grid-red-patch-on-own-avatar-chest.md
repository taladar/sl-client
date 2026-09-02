---
id: test-fake-grid-red-patch-on-own-avatar-chest
title: A red shard sits on the own avatar's chest in every fake-grid capture
topic: test
status: wont-do
origin: Firestorm capture run (2026-09-02)
points: 2
refs: [test-fake-grid-self-avatar-baked-textures-rejected, test-firestorm-crosscheck-report]
---

Context: [context/testing.md](../context/testing.md).

**Not a defect. It is the scene, read wrong.**

Once the own avatar de-clouded and stood up
([[test-fake-grid-self-avatar-baked-textures-rejected]]), captures showed
a dark-red, sharp-edged shard at chest height on the avatar, in the same
place in every frame — the only thing on the avatar that was not the
green the fake grid bakes it.

It is the **catalogue's red-and-green checker, seen through the gap
between the avatar's arm and its torso.** The camera used
(`128,120,27.5` → `128,128,26.2`) puts the prim row 8 m directly behind
the avatar, and `sculpt-sphere` stands at `x = 128`, dead centre. The
checker's *green* squares are indistinguishable from the green body and
the green ground, so only the red squares register — as a thin sliver
that reads like a shard stuck in the chest rather than like a glimpse of
something behind.

Settled by re-running the identical camera against `--scenario stock`,
which has no checker anywhere: **zero** reddish pixels within the
avatar's bounding box, and a clean silhouette. Everything else about the
frame is unchanged.

Two things worth keeping from the chase, because both would have been
believed:

- Every candidate that made it look like an avatar defect was checked
  and cleared first — no object within 4 m but the water plane, no
  wearable-layer stand-in that is red, the `IMG_DEFAULT_AVATAR` sentinel
  is grey. All true, and all irrelevant: the red was 8 m *behind* the
  avatar, outside every radius being searched.
- Brightening the crop is what solved it. At the captured exposure the
  shard looked like a solid shape on the body; at 3× it is plainly a
  sliver along the arm/torso silhouette, which is a shape only something
  seen *through a gap* can have.

The lesson for [[test-firestorm-crosscheck-report]]: a fixture scene
where the content and the ground share a colour makes "on the avatar" and
"behind the avatar" hard to tell apart. Judge a suspected avatar artefact
from a camera with nothing behind the subject before believing it.
