---
id: viewer-posed-avatar-bounds-rewritten-every-frame
title: The GPU-posed avatar Aabb is rewritten every frame, waking every consumer of it
topic: viewer
status: ideas
origin: noticed costing the water-clip straddle test while fixing
  viewer-skinned-bind-group-quits-on-rez (2026-09-07)
refs: [viewer-skinned-bind-group-quits-on-rez]
---

Context: [context/viewer.md](../context/viewer.md).

`gpu_avatars::stage::apply_gpu_avatar_bounds` writes each GPU-posed face's
bounds unconditionally:

```text
match existing {
    Some(mut existing) => *existing = aabb,
    None => commands.entity(entity).insert(aabb),
}
```

`*existing = aabb` marks the `Aabb` **changed every frame**, for every skinned
face of every avatar in view, whether or not the posed bound actually moved — a
still avatar, a paused animation and a fully settled crowd all pay it.

That is not wrong, but it wakes every `Changed<Aabb>` consumer in the engine
each frame. `crate::water_clip`'s straddle test is one such consumer and now
deliberately reads `aabb.is_changed()` (it has to: a mesh body's geometry moves
while its entity stands still), so it re-tests every avatar face every frame.
That particular cost is small — a handful of float operations that short-circuit
before the material lookup — but it is paid by anything else that keys off the
bound, and by anything that starts to.

## The change

`set_if_neq` (or an epsilon compare — the bound is `f32`, read back from the
GPU, and will jitter in the last bits even when nothing moves) so an unchanged
bound does not mark the component. A real animation still writes every frame,
which is correct; a still avatar stops.

## What to check

- Whether the read-back bound is bit-stable for a still avatar at all. If it
  jitters, `set_if_neq` buys nothing and the compare wants a tolerance — and
  picking that tolerance is the whole task, since too coarse a one would pin a
  slowly drifting bound.
- Who else reads `Changed<Aabb>`: frustum culling, `MeshRayCast` picking, the
  water clip. Each is a beneficiary, and each is a way to notice if the
  tolerance is too coarse.

Not urgent: nothing is known to be slow because of it. Filed because it was
measured in passing and is the kind of per-frame wake-up that gets expensive
quietly, as consumers accumulate.
