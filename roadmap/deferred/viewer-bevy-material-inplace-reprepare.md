---
id: viewer-bevy-material-inplace-reprepare
title: Bevy material in-place re-prepare (fast path for buffer-only changes)
topic: viewer
status: deferred
origin: discovered 2026-07-26 profiling the custom-face-material perf regression
refs: [viewer-custom-face-material-shader, viewer-profiling-logplugin-tracing]
---

Context: [context/viewer.md](../context/viewer.md).

Bevy re-prepares a material on **any** change by fully freeing and recreating
its bind group — there is no in-place fast path for a buffer-contents-only
change. In `bevy_pbr` 0.19 `material.rs`
(`RenderMaterialInstances::prepare_asset`, the `Entry::Occupied` arm) this is an
explicit TODO:

```text
Entry::Occupied(mut occupied_entry) => {
    // TODO: Have a fast path that doesn't require recreating the bind
    // group if only buffer contents change. For now, we just delete and
    // recreate the bind group.
    bind_group_allocator.free(*occupied_entry.get());
    let new_binding =
        bind_group_allocator.allocate_unprepared(unprepared, &material_layout);
    *occupied_entry.get_mut() = new_binding;
    new_binding
}
```

So every `Assets::get_mut` on a material re-registers **all** its bindless
resources (textures, samplers, data) into the slab, even when only a few bytes
of uniform data changed. This is cheap-ish for a bare `StandardMaterial` but
much heavier for [[viewer-custom-face-material-shader]]'s
`ExtendedMaterial<StandardMaterial, SlFaceExt>` (the base's whole binding set
**plus** the extension), so any per-frame material mutation is costly — the root
cause of the animated-texture perf regression.

We worked around the dominant source by moving **texture animation to the GPU**
([[viewer-custom-face-material-shader]] `sl_animated_uv`, so a running animation
dirties no material per frame). But other legitimate per-frame material churn
remains — media-video surfaces (`pump_media_engine`), texture LoD re-decodes /
streaming (`apply_prim_textures`, `compose_face_material`) — each paying the
full recreate cost.

**Do (deferred):** implement Bevy's own TODO — an in-place update path in the
`MaterialBindGroupAllocator` / `MaterialBindGroupBindlessAllocator` that, when a
re-prepared material's bindings are structurally identical to the previous ones
(same textures/samplers, only `#[data]` / uniform bytes changed), patches the
data buffer in the existing slab slot instead of `free` + `allocate_unprepared`.
This is a systemic win: it makes **all** per-frame material mutation cheap
(animation, media, streaming, future effects) and removes the incentive to avoid
mutating materials at all. It is a `bevy_pbr` core change, so per the
`sl-client-fork-upstream-for-upstream-bugs` memory (fork upstream for upstream
bugs) it means a `[patch.crates-io]` fork + an upstream PR (the TODO is already
flagged in-tree, so upstream is receptive). Larger and riskier than the
GPU-animation workaround, hence deferred.

Profiling to confirm the win needs render-world timing, which is currently
blocked — see [[viewer-profiling-logplugin-tracing]].
