---
id: viewer-attachment-earrings-not-rigged
title: Worn rigid attachments freeze at the T-pose instead of following the
  animated joint (earrings drift off the head)
topic: viewer
status: done
origin: user report during viewer-facelight-too-bright replay review (2026-08-06)
refs: [viewer-facelight-too-bright]
---

Context: [context/viewer.md](../context/viewer.md).

On the captured avatar replayed for
[viewer-facelight-too-bright](../done/viewer-facelight-too-bright.md) (bundle
agent `52ed4c6a`), a pair of **earrings** — plus a small **brand-label mesh**
between them — rendered beside the head, drifting ~2 heads off and **not moving
with the head as the avatar swayed** (sitting where the ears are in the T-pose).

## Done (2026-08-06)

Not a rigging problem (the original title's guess). The earring mesh
(`356817c3`) carries **no skin block** (confirmed from the LLMesh header) — it
is a genuinely **rigid** attachment worn on the Left / Right Ear attachment
points (ids 13 / 14), which hang off `mHead` at the `avatar_lad.xml` offset. So
its placement depends entirely on the attachment-point node's world transform.

Root cause: the pose driver
[`pose_avatar_skeletons`](../../sl-client-bevy-viewer/src/animations.rs)
overwrites each skeleton joint's `GlobalTransform` **directly** in `PostUpdate`,
*after* Bevy's transform propagation (P18.3 — the SL matrix-palette recurrence
produces world matrices, and GPU skinning reads them). It hand-re-places the
rigid **base parts** (the eyeballs) from their posed joint, but **not** the
attachment-point nodes — so every rigid worn attachment kept the stale global
propagation had computed from the joint's *pre-animation* (rest / T-pose)
transform, and Bevy's dirty-bit propagation never recomputes it (its ancestor's
global was overwritten out from under it). Hence the earrings froze at the
T-pose ear position and ignored the head animation.

Fix: a new `pose_attachment_nodes` system (runs right after the driver) marks
each attachment-point node (`AttachmentPointNode`) and, per node, reads its
posed parent-joint global and walks the node's subtree composing
`global = parent_global × local` down to the worn geometry — a targeted
mini-propagation of just the subtrees the direct-global-write orphaned. Fixes
**all** rigid worn attachments on animated joints (earrings, piercings, rigid
hats), not only this avatar's. Unit-tested (the subtree lands at the posed
joint, not the rest global) and confirmed live in replay: the earrings now track
the head as it sways.

An unrelated hair transparency issue observed on the same avatar is tracked by
[mesh-hair-and-hairbase](../bugs/viewer-avatar-mesh-hair-and-hairbase-both-render.md).
