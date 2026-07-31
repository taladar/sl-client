---
id: viewer-perf-avatar-bake-apply-spikes
title: Batch / defer avatar bake + skeleton application to smooth per-frame spikes
topic: viewer
status: done
origin: Tracy profiling of Aditi rezzing (2026-07-31, 2-min capture)
refs: [viewer-perf-skeleton-single-solve]
---

Context: [context/viewer.md](../context/viewer.md).

A full 2-minute Tracy capture of aditi rezzing (4511 frames) shows several of
**our own** avatar systems as single-frame outliers that fire when an avatar
loads / (re)bakes. Their means are tiny (they idle most frames), but the
worst frame is large — a visible hitch when an avatar appears or rebakes:

| System (`sl_client_bevy_viewer::…`) | peak | mean |
| --- | --- | --- |
| `avatars::apply_own_local_bake` | 55 ms | 0.02 ms |
| `avatars::apply_avatar_bake_textures` | 39 ms | 0.03 ms |
| `animations::drive_avatar_skeletons` | 34 ms | 0.47 ms |
| `objects::apply_rigged_attachments` | 21 ms | 0.11 ms |
| `avatars::apply_bom_face_materials` | 18 ms | 0.50 ms |

`apply_own_local_bake` is ~one big event (68 % of its whole total is a single
call): the own-avatar bake composite is applied in one frame. The others fire
in bursts as other avatars stream in and bake.

Investigate:

- Whether the bake-application systems can **spread work across frames** (apply
  one avatar's / one region's bake per frame, or time-slice the composite)
  instead of doing all pending bakes in a single frame.
- Whether `drive_avatar_skeletons` (mean 0.47 ms, spikes to 34 ms) can cap the
  number of skeletons solved per frame, or skip distant / off-screen avatars —
  see [[viewer-perf-skeleton-single-solve]].
- Whether any of these re-run redundantly (e.g. re-applying an unchanged bake)
  — the "build once, update in place" discipline applies here too.

## `apply_own_local_bake` root cause + fix (confirmed 2026-07-31)

The spike is the **client-side bake composite** (`build_local_bake` →
`sl-bake::composite_region`): for each of ~5–6 body regions (`BakeRegion::ALL`)
it blends the wearable layers (skin + tattoos + each clothing layer + alpha
masks) into one `LOCAL_BAKE_SIZE = 512`² image, plus a per-region V-flip and an
alpha-classification pass — all **single-threaded, in one frame** (`sl-bake` has
no rayon/async; `build_local_bake` runs once when the inputs finish assembling).
It is *not* a texture upload — the `images.add` at the end is the cheap part.

Two fixes, composing:

1. **Gate it on the server bake — at the input-fetch level, not just the
   composite.** On SL / BoM our own avatar has a full server bake, which wins in
   the draping loop (`state.baked_textures` per region), so the whole composite
   is **built then discarded** — pure waste. Yet the only guard before
   `build_local_bake` is `!local.built`; nothing checks our own agent's
   `baked_textures` first. Gate earlier still: the local bake needs every
   *layer* texture fetched + JPEG2000-decoded, whereas the server bake is one
   already-composited texture per region — so assembling the local-bake inputs
   is strictly *more* asset traffic + decode than the bake it supersedes and
   cannot even show the avatar sooner. So on a server-bake avatar, don't fetch
   the layer textures / assemble `OwnBakeInputs` for compositing at all. (Keep
   the genuinely separate needs: wearable *params* for shape resolution in
   `apply_own_shape_from_wearables`, and the inputs the appearance editor
   mutates during a live edit.)
2. **Background-worker the composite that remains (OpenSim).** Where no server
   bake exists the composite is real; run it on an `AsyncComputeTaskPool` task
   and install the resulting `Handle<Image>`s on completion instead of blocking
   the frame thread — pure pixel work, no ECS access until the handles are
   ready, and a one-shot (`local.built`) so there is no per-frame coordination.
   rayon over the ~6 regions / scanlines is a further near-linear speedup.

So the gate removes the cost where it is pure waste (SL), and the worker removes
the *stall* where the work is real (OpenSim).

Measure each system's per-event max (not just mean) before/after with a
multi-minute capture while avatars are loading (the mean hides these; only the
per-event spike distribution shows them — see `book/src/tools/profiling.md`).

## Done (2026-07-31) — `apply_own_local_bake` only

Both confirmed fixes for `apply_own_local_bake` (the 55 ms outlier) landed:

1. **Gate the client composite on the server bake.** `OwnBakeInputs` now latches
   a `server_bake_grid` flag from the `UpdateAvatarAppearance` capability; on a
   server-bake grid the layer textures are not fetched and the composite is not
   assembled (`bake_inputs.rs`), so the ~55 ms `build_local_bake` never runs on
   SL. The wearable *assets* are still parsed for shape params, and the
   appearance editor still composites on demand. Verified on aditi:
   `composited client-side bake` fires 0 times (was ≥1).
2. **Off-thread the composite that remains (OpenSim).** `build_local_bake` is
   now a background `AsyncComputeTaskPool` job (`run_local_bake_job`), polled
   non-blocking with `poll_once` — the frame is never stalled; the composited
   images install a later frame. A newer `OwnBakeInputs` generation supersedes
   an in-flight one. Verified on OpenSim (composites run on the task pool, no
   panic).

This also matters for the runtime re-bake
([[viewer-own-bake-not-refreshed-on-outfit-change]]): without the gate, every SL
outfit change would re-fire the wasted composite.

**Still open (moved out of scope for this pass):** the task's other
*Investigate* items — spreading **other** avatars' bake application
(`apply_avatar_bake_textures`, `apply_bom_face_materials`) across frames, and
capping / off-screen-skipping `drive_avatar_skeletons`. A full Tracy
before/after of the per-event max distribution was not re-run.
