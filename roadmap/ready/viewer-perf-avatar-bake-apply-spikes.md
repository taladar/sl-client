---
id: viewer-perf-avatar-bake-apply-spikes
title: Batch / defer avatar bake + skeleton application to smooth per-frame spikes
topic: viewer
status: ready
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

Measure each system's per-event max (not just mean) before/after with a
multi-minute capture while avatars are loading (the mean hides these; only the
per-event spike distribution shows them — see `book/src/tools/profiling.md`).
