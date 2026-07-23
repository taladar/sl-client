---
id: viewer-transform-gizmos
title: Position / rotation / scale gizmos
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-object-selection-core, viewer-input-action-map]
---

Context: [context/viewer.md](../context/viewer.md).

Interactive manipulators for the selected object(s) in the selection set
([[viewer-object-selection-core]]): move (3-axis + planar handles), rotate
(rings), and stretch / scale handles, with grid snapping and local / world /
reference frames. The manipulators consume input **actions**
([[viewer-input-action-map]]) rather than raw mouse buttons. Edits are pushed to
the sim via `ObjectUpdate` / `MultipleObjectUpdate`.

Reference (Firestorm, read-only): `llmaniptranslate`, `llmaniprotate`,
`llmanipscale`, `llmanip`.

## Done

`sl-client-bevy-viewer/src/gizmos.rs` + the pure drag math in
`edit_math.rs` (unit-tested: ray/plane, camera-facing manip plane, ring
angles, closest-line params, grid/angle snapping, scale clamps, Euler
round-trips). Move (two-headed axis arrows + planar pads), rotate (axis
rings), and stretch (face handles single-axis in the primary's own frame,
corner handles uniform about the pivot) over the whole selection set, with
the reference's cursor-distance **snap regimes and white snap guides** on
all three tools: the translate ruler (tape-measure-graded ticks on the
absolute grid), the rotate detent circle (5.625° detents, long thick 90°
cardinals), and the stretch rulers (absolute size grid plus ×0.5-factor
marks with ×1.0 largest; corners a quarter-factor ladder) — each with a
sliding held-mark highlight, engaging only once the cursor crosses the
guide — and world / local frames
(`Reference` modelled for [[viewer-build-grid-options]]). The rig holds a
constant screen size and draws through its own overlay camera (render layer
3, order 1, between world and HUD) so it is never occluded. Drags apply
locally each frame (the `ObjectSlMotion` echo keeps the numeric fields live)
and send `MultipleObjectUpdate` on release — stretch also streams at the
reference's 10 Hz — with the linked-set / uniform flags; linked-part edits
fold through the parent's frame. Deliberate deviations, documented in the
module: no free-rotate sphere, no copy-on-drag, and the manipulators read
the pointer directly (the action map is keyboard-only).

Refined during the live review (this session, reference-checked): rotation
snapping quantises the object's **absolute twist** about the ring axis
(swing–twist decomposition; repeatable 0°/90°/… whatever the grab point)
with a continuous wrap-free accumulated angle; the stretch tool mounts its
handles on the live **selection bounding box** (world AABB in the world
frame, the primary's box in local — `getBBoxOfSelection`) with a wireframe,
folds each object's stretch onto its **nearest local scale axis** divided by
the alignment (`stretchFace`'s `nearestAxis`), and honours both
stretch-both-sides modes (centre-scaling vs. opposite-face / opposite-corner
with the halved `0.5 + t/2` cursor mapping) plus the reference's shared
corner-factor clamp; and a live value read-out (position / degrees / size /
factor) rides beside the gizmo during every drag. The stretch box is
grid-frame-aligned and, during a face drag, its **display** changes on the
dragged axis alone (grabbed side follows the cursor, the opposite side
pinned — or mirrored with stretch-both-sides, which doubles size per cursor
travel), re-fitting the live selection only between drags.
