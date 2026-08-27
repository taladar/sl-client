---
id: protocol-audit-mesh-joint-index-bounds
title: A rig's joint indices are never checked against the skin's joint count
topic: protocol
status: bugs
origin: static code audit (2026-08-26); split from protocol-audit-mesh-decode-allocation-caps
points: 2
refs: [protocol-audit-mesh-decode-allocation-caps]
---

Context: [context/protocol.md](../context/protocol.md).

`decode_weights` (`sl-mesh/src/decode.rs`) accepts any `u8` joint byte below
the `0xFF` sentinel. Nothing checks it against the number of joints the mesh's
skin block actually declares.

The check cannot live in `decode_weights`: the weights are in a geometry LOD
block and `MeshSkin::joint_names` is in the `skin` block, so the two never meet
inside `sl-mesh`. It has to happen at the consumer that joins them — and
`sl-client-bevy/src/meshes.rs`'s `pack_influences` does not do it either, which
is the actual gap.

Not a crash: the workspace denies `indexing_slicing`, so an out-of-range joint
resolves through a `.get()` rather than panicking. It is a correctness bug —
an influence silently binds to the wrong bone, or to none.

Scope: validate at the join, dropping (not clamping) an influence whose joint
index is past the skin's `joint_names`, on the same reasoning as the triangle
indices in [[protocol-audit-mesh-decode-allocation-caps]] — a clamped index
welds a vertex to an unrelated bone.
