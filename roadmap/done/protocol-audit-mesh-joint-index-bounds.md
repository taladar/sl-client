---
id: protocol-audit-mesh-joint-index-bounds
title: A rig's joint indices are never checked against the skin's joint count
topic: protocol
status: done
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

## Fixed (2026-08-28)

`to_bevy_rigged_mesh` now takes the `&MeshSkin` it is already paired with at
both call sites (`sl-viewer-world-avatar`'s `build_rigged_submeshes` and the
render-test scene), and `pack_influences` keeps only influences naming one of
the skin's declared joints.

Two things the check had to get right beyond "skip it":

- The skip happens **before** a slot is filled, not after. Filtering into the
  four slots positionally would let a bogus first influence cost the vertex one
  of its four real ones.
- The existing renormalization then does the rest: a vertex that loses half its
  weight to a dropped influence renormalizes onto what remains, and a vertex
  that loses *all* of them falls into the pre-existing sum-zero path and binds
  fully to joint 0 rather than collapsing to the mesh origin under an all-zero
  skinning matrix.

The bound is exactly right at this join because the same `joint_names.len()`
sizes the `SkinnedMesh::joints` entity list and the `canonical` palette map
built next to it — an index past it resolved to nothing in both.

Two tests: the drop (in three shapes — bogus alongside real, bogus first, all
bogus) and a guard that five *valid* influences still keep only their first
four, so the new filter did not quietly turn the four-slot limit into
"the first four survivors".

Not touched: the base-body path (`sl-client-bevy/src/avatars.rs`
`build_base_mesh`), which clamps rather than drops. Its weights come from the
local `avatar_lad` character files, not from an asset off the CDN, so it is not
the same hostile-input surface.
