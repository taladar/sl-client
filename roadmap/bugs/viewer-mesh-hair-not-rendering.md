---
id: viewer-mesh-hair-not-rendering
title: Some worn mesh hair does not render (visible in Firestorm)
topic: viewer
status: bugs
origin: user report during viewer-avatar-tongue-protrudes aditi testing (2026-08-05)
---

Context: [context/viewer.md](../context/viewer.md).

An avatar's worn **mesh hair** that Firestorm renders is **not rendered at all**
in our viewer (observed live on aditi — one avatar's hair simply missing while
the rest of the avatar draws).

Investigate why that specific worn mesh (hair attachment) is skipped: candidates
— its mesh LOD/asset never fetched or decoded (a decode error dropping the
submesh), all its faces classified fully transparent (an alpha-mode /
transparent-material misclassification hiding it), or a rigged-attachment bind
that silently drops it. Identify the hair asset id live, decode it offline, and
check the face materials / decode path. Distinct from the fully-transparent
box-shell animesh issue but worth cross-checking the transparent-face policy.
