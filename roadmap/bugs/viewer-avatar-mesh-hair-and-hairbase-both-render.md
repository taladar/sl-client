---
id: viewer-avatar-mesh-hair-and-hairbase-both-render
title: Avatar shows mesh hair and a legacy hairbase at the same time
topic: viewer
status: bugs
origin: user report during viewer-facelight-too-bright replay review (2026-08-06)
refs: [viewer-facelight-too-bright]
---

Context: [context/viewer.md](../context/viewer.md).

On the captured avatar replayed for
[viewer-facelight-too-bright](../done/viewer-facelight-too-bright.md) (bundle
agent
`52ed4c6a`), the avatar renders **mesh hair** *and* what looks like a **legacy
classic-avatar hairbase** at the same time — the two overlap instead of the mesh
hair replacing the system hair.

Real viewers hide the system-avatar hair (and the base-body scalp) when a mesh
hair attachment / Bake-on-Mesh alpha wants it gone. This is likely the same
class of "hide the system body region a worn item covers" mechanism as the R22g
alpha-layer `IMG_INVISIBLE` bake work — but for the **hair** region /
`hair`-mesh part, which may not be honouring the worn wearable's alpha or the
mesh-hair presence. Investigate: is the base-body `hair` mesh part (or the
hairbase texture layer) being suppressed when a mesh-hair attachment is worn,
and does the avatar's alpha layers / `param_alpha` masks cover the scalp?

Reproduce offline via the stored dump (`--replay`, frame the head). Compare the
base-body part list and the worn-attachment set against Firestorm's rendering of
the same avatar.
