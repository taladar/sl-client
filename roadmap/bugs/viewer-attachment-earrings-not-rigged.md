---
id: viewer-attachment-earrings-not-rigged
title: Worn earrings (and a brand-label mesh) float beside the head instead of
  rigging to the ears
topic: viewer
status: bugs
origin: user report during viewer-facelight-too-bright replay review (2026-08-06)
refs: [viewer-facelight-too-bright]
---

Context: [context/viewer.md](../context/viewer.md).

On the captured avatar replayed for
[viewer-facelight-too-bright](../done/viewer-facelight-too-bright.md) (bundle
agent
`52ed4c6a`), a pair of **earrings** — plus a small **brand-label mesh** between
them — render **floating beside the head** rather than sitting on the ears.

Likely a rigged-attachment skinning / joint-resolution issue: the earrings are
rigged mesh that should bind to the ear collision volumes / head joints, but
either the rig binds to the wrong joint (falling back to pelvis / avatar
centre), or the attachment is being placed by its attach-point node transform
without honouring its skin weights. Compare against the P17.2 rigged-mesh
binding path (`bound rigged mesh … to its skeleton`, and the
`N/M joint(s) unresolved, bound to pelvis` diagnostic) and the collision-volume
joints (`L_EAR` / `R_EAR` and the head bones) exposed by
`BevySkeleton::from_skeleton`.

Reproduce offline via the stored dump:
`--replay avatar-dumps` (or a focused single-manifest bundle for `52ed4c6a`),
then frame the head. `RUST_LOG=…::objects=debug` logs each rigged-mesh bind and
any unresolved joints.
