---
id: build-split-world-avatar-crate
title: Separate the avatar layer from the object layer in the world split
topic: viewer
status: ideas
origin: crate-split work (2026-08) — the four-way world split reduced to three
refs: [build-split-viewer-crate, viewer-ecs-idiom-audit,
  build-structural-encapsulation-audit]
---

Context: [context/viewer.md](../context/viewer.md).

The world was split three ways -- objects, scene, view -- and not the four the
plan drew, because objects and avatars would not come apart. This is what it
would take to finish the job, and why it was left.

Measured at the time: the four-way grouping had five cyclic pairs and 57
cross-group references; merging objects and avatar left two cycles and 44, both
of which were then broken. Re-measure before starting -- the numbers below are
from that snapshot and the layout has moved since.

## Why they would not separate

Not an accident of layering. In Second Life an avatar *is* an object, an
attachment is an object parented to an avatar, and several viewer subsystems
legitimately serve both:

- **The asset managers.** `TextureManager` and `MeshManager` live with the
  objects and are *called* by the avatar side -- to request a bake layer, to
  read a decoded texture, to size a rigged mesh for the complexity budget.
  Fetch caches with work queues, so they cannot move down to `world-api`; the
  fix is to invert them, publishing decoded results the consumer reads rather
  than answering calls. This is the same change [[viewer-ecs-idiom-audit]]
  describes for managers generally, and it is the largest single piece.
- **The billboard renderer.** `name_tag_billboard` and `name_tag_content` sit
  with the avatars, but `hover_text` -- an object's `llSetText` -- renders
  through the same world-space billboard. It is shared infrastructure filed
  under one of its two users.
- **Derender.** Lives with the objects and hides avatars too, so it reaches
  for the avatar placeholder assets and the per-agent derender path.
- **Rigged attachments.** `objects.rs` knows about `AvatarBody`, the BOM face
  materials, the GPU skin binding and the animesh control avatar, because a
  worn mesh is an object that is rigged to a skeleton.

## What it would take

1. Invert the two asset managers (the bulk of the work, and worth doing for its
   own sake -- see [[viewer-ecs-idiom-audit]]).
2. Give the billboard renderer a home that is neither avatar nor object, or
   move it to whichever side keeps the graph acyclic.
3. Decide where the avatar half of derender belongs.
4. Move the small pure items that were left alone because nothing required them
   -- the scene-object marker and the animesh root walk among them.

## Is it worth it

Not obviously, which is why it is `ideas` and not `ready`. The three-way split
already turned one 68k-line compilation unit into roughly 40k / 17k / 11k, and
most of the rebuild-scope win came with it. A fourth crate would divide the
40k, and the plan's own churn figures say the avatar modules are edited often
enough for that to matter -- but the manager inversion is a real redesign of
the asset pipeline's direction, not a relocation.

Do it when the manager inversion is wanted anyway. Doing it *only* to reach
four crates would be paying a redesign for a line-count target.
