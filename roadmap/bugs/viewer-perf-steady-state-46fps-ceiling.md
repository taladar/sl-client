---
id: viewer-perf-steady-state-46fps-ceiling
title: Steady-state frame rate caps at ~46 fps on the local grid (was 60)
topic: viewer
status: bugs
origin: A/B verification runs for the run-condition gating pass (2026-08-10)
refs: [viewer-profiling]
---

Context: [context/viewer.md](../context/viewer.md).

During the [[viewer-perf-inventory-view-visibility-gate]] verification
(2026-08-10, `performance` branch, local OpenSim, 3840x2160 window — the
same screen earlier sessions used), the viewer never reached 60 fps:
~44 fps at t=40 s and still ~46 fps at t=95 s, well past the cold-login
streaming burst. Past runs on this scene reportedly sat at 60 fps.

Ruled out by measurement (traces in that session's scratchpad, numbers
in the two gating commits):

- **The visibility-gating change**: alternating gated/ungated A/B runs
  (`SL_VIEWER_DISABLE_PANEL_GATE` toggle, same binary, same conditions)
  show identical visible-phase fps (46.7 / 47.1 / 45.7) and identical
  Render-schedule medians (19.78 / 19.78 / 20.20 ms).
- **Tracy overhead**: with no profiler attached the status-bar readout
  still shows 44-46 fps.
- **Compositor present-throttling**: distinct symptom (~1000 ms
  `vkQueuePresentKHR` blocks while the window is occluded); the 46 fps
  phases have 0.1 ms presents.

Steady-state frame anatomy (visible phase, t=30-60 s, gated run b2):
both threads sit at ~21 ms, so the frame is co-limited —

- main thread: `Main` schedule ~14.8 ms (PostUpdate alone 6.6 ms) +
  RenderExtractApp ~6.9 ms ≈ 21.7 ms;
- render thread: `Render` schedule ~20.2 ms (render_system 7.4 ms,
  camera_driver 4.2 ms, the rest parallel prepare/queue work, e.g.
  `queue_shadows` ~2.3 ms across many workers).

## Aditi steady-state anatomy (full-session Tracy capture, 2026-08-10)

A whole-session `tracy-capture` on **aditi** (`--features profile-tracy`,
release, 3424 frames / 5:09, 25.3 M zones, clean disconnect) re-measures
the same anatomy on a **denser** scene (full region + several avatars).
The first ~135 s were compositor-throttled (window occluded during
login/rez → 141 ~1000 ms `vkQueuePresentKHR` frames); all numbers below
are the **visible steady state** (frame deltas from consecutive `finished
frame` marks, `t ≥ 140 s`, 3192 frames):

- frame: mean **49.9 ms (20 fps)**, p50 **45.0 ms**, p95 84.3 ms, p99
  115.6 ms, max 428 ms.
- `schedule{name=Main}` (thread 1) mean **45.9 ms / p50 42.4 ms** — this
  *is* the frame.
- `schedule{name=Render}` (thread 2, concurrent) mean **33.3 ms** — it
  has **headroom**. Unlike the local-OpenSim capture above (co-limited at
  ~21/21 ms), aditi is squarely **main-thread bound**: GPU/render is not
  the bottleneck, CPU main-thread work is.

Main splits into PostUpdate 22.3 ms (p50 21.9, rock-steady → the median
floor) + Update 17.1 ms (p50 11.9, spiky → the tail) + ExtractSchedule
3.0 ms + PreUpdate 1.9 ms. The two heavy schedules broken down (per-frame
means; the specialization/visibility systems run on **main-app worker
threads**, so their wall-clock contribution to PostUpdate's 22 ms is the
critical chain, not the raw sum):

**PostUpdate — the steady median.** ⚠️ **Corrected 2026-08-10:** the
material-spec row below is a **sum of concurrent `par_iter` systems** — a
`-u` unwrap shows all 15 finish inside a ~2 ms overlapping span, so its real
wall-clock is **~2 ms, not 8.44 ms** (the summed-parallel-work over-count).
`check_dir_light_mesh_visibility`, by contrast, is a **single serial** system
at **~5–6 ms** real wall-clock — the genuine top single-threaded cost. A
proper critical-path breakdown (per-instance wall-clock, single-threaded vs
`par_iter`) is still owed; the "8.4 + 8.2 = 17 of 22" claim below is **wrong**
(it summed concurrent work).

| Cluster | summed ms | real wall-clock | note |
| --- | --- | --- | --- |
| `check_dir_light_mesh_visibility` (+ commands) | 8.18 | **~5–6 ms serial** | sun shadow-caster visibility — the real target, [[viewer-perf-pbr-shadow-cluster-rez]] |
| `material::check_entities_needing_specialization<M>` ×15 | 8.44 | **~2 ms (parallel)** | 15 concurrent `par_iter` systems — [[viewer-perf-main-world-material-specialization-check]] (downgraded) |
| `calculate_bounds` | 2.06 | ~2 ms | AABB recompute for changed geometry |
| everything else (propagate, change-detect, UI layout, text) | — | small | each ≤1.2 ms |

**Update — the spiky tail:**

| System | ms/frame | max | note |
| --- | --- | --- | --- |
| `ground::probe_avatar_ground` | **6.14** | 169.7 ms | tall pole + top spike — [[viewer-perf-avatar-ground-probe]] |
| `parcel_borders::update_parcel_borders` | 3.11 | 45.8 ms | p50 0 (only on parcel change) |
| `hover_tooltip::update_hover_tooltip` | 1.93 | 39.2 ms | p50 0 (spiky) |
| `objects::apply_prim_lod` (commands) | — | 358.2 ms | p50 0; biggest single spike |

Render thread (non-gating today): `render_system` 12.6 ms, `camera_driver`
7.0 ms, `specialize_shadows` 4.2 ms, `queue_shadows` 3.8 ms. These matter
only if PostUpdate is cut enough that render becomes the gater.

**Corrected takeaway:** once parallel work is discounted, the material
checks are ~2 ms wall-clock, not a dominant cost. The clearest real
average-frame lever in PostUpdate is the **single-threaded**
`check_dir_light_mesh_visibility` (~5–6 ms). What fills the rest of the
22 ms is a **chain of many smaller serial systems** (transform propagation,
visibility, change-detection, extract-prep, UI/text, our own) — no other
single dominant cluster — so beyond the shadow system this is death-by-many
and needs a critical-path (not summed-self-time) redo. Ground-probe (6 ms,
Update) was the biggest *spike* source and is now addressed
([[viewer-perf-avatar-ground-probe]] Stage 1).

### Outlier frames (why some frames are much slower)

The ~53 visible outliers (100–428 ms) cluster in the active rez/camera
window (t ≈ 156–195 s, 231 s, 263 s) and have three causes:

- **Prim/mesh landing bursts** — `apply_prim_lod` command flush (up to
  **358 ms** @231 s) firing alongside `allocate_and_free_meshes` (51 ms)
  and `collect_meshes_for_gpu_building` (42 ms): a batch of prims
  rezzing/LOD-swapping in one frame spawns+despawns entities and rebuilds
  GPU mesh buffers together.
- **Ground-probe spikes** — `probe_avatar_ground` **169 ms** @164 s when avatars
  land/move (the full-scene raycast, [[viewer-perf-avatar-ground-probe]]).
- **Texture landing** — `extract_render_asset<texture>` max 140 ms,
  `prepare_assets<gpu_image>` 73 ms, `apply_prim_textures` 75 ms as
  decoded textures upload.
- Occasional: `apply_bom_face_materials` 59 ms (avatar bake), `text_system`
  63 ms, `update_parcel_borders` 46 ms (parcel crossing).

## Investigation plan

- Establish when it regressed: rerun the same measurement (status-bar
  fps + a tracy capture, window visible) on earlier `performance`-branch
  commits — candidates since the last known-60 observation include the
  terse-update fast path, the bevy_flair patch pin, the session network
  thread, and the frame-spreading pass — and bisect the first ~21 ms
  commit.
- Attack the two PostUpdate levers first (biggest, steady, main-thread):
  main-world material-specialization gating
  ([[viewer-perf-main-world-material-specialization-check]]) and
  directional shadow-caster visibility
  ([[viewer-perf-pbr-shadow-cluster-rez]] item 3), then the ground probe.
- Confirm any fix at the status bar (60 fps restored) AND in the trace
  (Main / PostUpdate / Update medians), window visible and focused.
