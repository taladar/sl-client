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

## Re-capture 2026-08-12 — both 2026-08-10 levers verified landed

A fresh full-session aditi `tracy-capture` (release, `profile-tracy`, 2180
frames / 2:51, 15.0 M zones, clean disconnect; 56 occluded ~1 s present
frames = 2.6 %, excluded) re-measures the anatomy after the ground-probe and
shadow-visibility work landed. Frame is unchanged at the top line — median
**49.3 ms (~20 fps)**.

### Critical path — the frame is CO-LIMITED, not main-bound

A per-instance reconstruction of a representative visible frame (Main start
→ next Main start) shows the period is captured, to **0.3 ms**, by:

```text
frame_period ≈ ExtractSchedule + max(Main-app, Render-app)
   49.3 ms    ≈     7.1 ms      + max( 41.4 , 39.6 )
```

- **Main-app (41.4 ms, thread 1)** and **Render-app (39.6 ms, thread 2)** run
  **concurrently** (pipelined) and are within ~2 ms; **RenderApp is the
  *longer* thread in 54 % of visible frames.** So the frame waits for whichever
  is slower and *both* are on the critical path — cutting only one floors at
  `extract + the other` (~47 ms). (An earlier draft here wrongly decomposed
  RenderApp as "Extract 7 + RenderGraph 16.5 = 24 ms"; that is **wrong** —
  `sub app{name=RenderApp}` does **not** contain ExtractSchedule and holds the
  whole render-world prepare/queue phase, ~21 ms, *plus* `render_system`
  ~19 ms.)
- **ExtractSchedule (7.1 ms) is fully serial** — a hard pipeline sync where the
  main thread blocks on the render thread finishing the previous frame, then
  copies the world across with **both threads otherwise idle**. Every ms cut
  here is a full ms off the frame. It is ~90 % **one system**: `extract_skins`
  5.4–7.6 ms (avatar joint matrices; scales with avatars × Bento bones) +
  `extract_lights` 1.4 ms.
- Main-app and render-app **share one worker pool**, so they contend for the
  same ~8 workers while overlapping — cutting either frees workers for the
  other.

The three critical segments' internal chains (representative frame):

- **ExtractSchedule 7.1 ms (serial):** `extract_skins` ≫ `extract_lights`.
- **Main-app 41 ms:** sequential barriers Update 17 + PostUpdate 16.5 +
  physics 4.8 + PreUpdate 2.3. PostUpdate chain =
  `mark_3d_meshes_as_changed` 3.6 → `calculate_bounds` 1.9 →
  material-spec ×15 ~2.8 (parallel) → `check_visibility_cpu_culling` 1.3 →
  `pose_avatar_skeletons` 1.1 → `ui_layout_system` 1.1 →
  `shadow_visibility` ~1.2 → transform propagation ~0.9. Update chain =
  `drive_render_priority` 4.5 (4 Hz throttle) + `update_hover_tooltip` 3.7
  (cursor-dependent) + `apply_prim_lod` 1.5 + `prune_control_avatars` 1.2 +
  a long tail of ~0-cost UI systems.
- **Render-app 40 ms:** prepare/queue ~21 ms (`prepare_skins` 3.2,
  `write_indirect_parameters_buffers` 2.8, `collect_visible_cpu_culled` 2.1,
  `queue_shadows` 1.2, `prepare_material_bind_groups` 1.2, probe bind groups
  1.1, `specialize_shadows`) + `render_system` ~19 ms (`camera_driver` 14.1 =
  the 3D + shadow + reflection-probe/hero passes; `submit_pending_command_
  buffers` 4.1).

**Lever order:** (1) `extract_skins` — best ms-for-ms, alone on the serial
segment; (2) attack Main *and* Render together (balanced at ~40 ms, shared
workers); (3) render scales with drawn objects / shadow casters / probes;
(4) PostUpdate is geometry-change bound (`mark_3d_meshes_as_changed` +
`calculate_bounds`). See [[viewer-perf-avatar-pose-extract-skins]] and
[[viewer-perf-name-tag-per-frame-churn]] for the concrete first cuts.

But the two biggest 2026-08-10 single costs are **measured gone**:

- **Ground probe:** `probe_avatar_ground` **6.14 ms → 0.074 ms**/frame (max
  169 ms → 2.5 ms) — the collision-plane rewrite
  ([[viewer-avatar-ground-from-collision-plane]], HEAD commit; supersedes
  [[viewer-perf-avatar-ground-probe]]) removed the full-scene raycast. 83×.
- **Shadow-caster visibility:** bevy's `check_dir_light_mesh_visibility`
  (~5–6 ms serial) is replaced by our `shadow_visibility` module —
  `mark_shadow_caster` 0.79 + `dispatch_shadow_casters` 0.37 +
  `apply_shadow_cull` 0.02 + `build_directional_light_cascades` 0.01 ≈
  **1.2 ms** total ([[viewer-perf-pbr-shadow-cluster-rez]]).

`PostUpdate` dropped **22.3 → 16.5 ms** accordingly, and is now confirmed
**death-by-many** (no single dominant serial cost): `calculate_bounds`
2.18, `mark_3d_meshes_as_changed_if_their_assets_changed` 2.14, the 15
`check_entities_needing_specialization<M>` ≈2 ms (parallel),
`check_visibility_cpu_culling` 1.56, `collect_meshes_for_gpu_building` 1.38,
`shadow_visibility` ~1.2, transform/visibility propagation ~0.7, rest ≤1 ms.

`Update` stayed ~17.4 ms only because, with the ground probe gone, the new
top system is **`update_hover_tooltip` at 5.9 ms** (a `MeshRayCast` over all
meshes, fired each dwelt frame — pointer was active this run; see
[[viewer-perf-hover-pick-raycast]]).

**Takeaway:** the two named levers delivered, but the frame is **co-limited
main/render + a 7 ms serial extract** (see the critical-path section above),
*not* main-bound. Reaching 60 fps needs all three of: the serial
`extract_skins` cut, the main-app chain (hover-pick + PostUpdate
death-by-many), and the render-app 3D/shadow/probe passes — no single 5 ms+
serial target dominates except `extract_skins` and (under active cursor use)
the hover-pick raycast.

### Re-capture outliers (2026-08-12)

Same classes as 2026-08-10, all in the rez/camera-move window (visible
outliers to 722 ms; ratios are max/mean over the session):

- **Prim/mesh landing bursts** — `update_objects` **261 ms**,
  `allocate_and_free_meshes` 68 ms, `collect_meshes_for_gpu_building` 17 ms
  in one frame as a batch of prims rezzes and GPU mesh buffers rebuild.
- **Terrain rebuild** — `update_terrain` **254 ms** (p50 0; only on a patch
  edit / region change).
- **Texture landing** — `prepare_assets<GpuImage>` 92 ms, 87 ms;
  `patch_parked_decoded_textures` 59 ms; `apply_prim_textures` 36 ms as
  decoded textures upload.
- **Avatar landing** — `apply_avatar_bake_textures` 111 ms;
  `apply_rigged_attachments` 110 ms.
- **Pipeline specialization stall** — `prepare_view_upscaling_pipelines`
  101 ms (one-time shader/pipeline compile).
- **UI text** — `apply_text_edits` 61 ms, `text_system` 61 ms,
  `measure_text_system` 49 ms (parley layout spikes on a text change).

## Investigation plan

- Establish when it regressed: rerun the same measurement (status-bar
  fps + a tracy capture, window visible) on earlier `performance`-branch
  commits — candidates since the last known-60 observation include the
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

## A lead found elsewhere (2026-09-25)

While chasing a slow gallery, `bevy_ui::layout::ui_layout_system` turned out to
re-lay-out most of the UI tree **every frame**: Bevy 0.19's
`update_editable_text_layout` flagged every `EditableText` as changed each
frame, so every text field's `ContentSize` was re-set and taffy re-laid-out
(and re-measured the text of) the field and all its ancestors. The cost scales
with how many text fields exist, hidden and not-yet-opened windows included —
the live viewer has this too. Fixed in our Bevy fork (`de3251e1`, see the
`[patch.crates-io]` comment); the gallery went from ~380 ms to ~25 ms of main
schedule per frame. The UI-text rows above (`text_system`,
`measure_text_system`) may be this. **Re-measure before anything else** — the
ceiling may have moved.

### Re-measured after the fork fix (2026-09-25)

Release `profile-tracy` build at Bevy fork `de3251e1`, whole-session
`tracy-capture`s, visible steady-state windows (last 60 s / 90 s, no
occluded frames):

| | local OpenSim (2299 frames) | aditi (1835 frames) |
| --- | --- | --- |
| frame p50 / mean / p95 | **20.6** / 25.7 / 30.9 ms (≈49 fps) | **44.7** / 49.0 / 86.0 ms (≈22 fps) |
| `Main` p50 | 16.3 ms | 36.4 ms (PostUpdate 19.8, Update 11.8) |
| `Render` p50 | 17.8 ms | 40.5 ms |
| `ExtractSchedule` p50 | 1.5 ms | 2.3 ms (was 7.1) |
| `vkQueuePresentKHR` p50 | — | 0.09 ms (no GPU / vsync wait) |
| `ui_layout_system` | 4.7 ms, ~every other frame | **4.7 ms every frame** |
| text systems | ≤0.13 ms | ≤0.2 ms (were 49–63 ms spikes) |

- The text-field fix removed the text-system costs, but the headline barely
  moved: aditi is still ~22 fps, OpenSim ~49 fps. The frame is still
  **co-limited, CPU on both threads**; on aditi `Render` is now the (slightly)
  longer one. `render_system` 18.8 ms p50 of which `camera_driver` (3D +
  shadow + reflection-probe pass encoding) 13.5 ms; the rest of `Render` is
  the render-world prepare/queue phase (~21 ms).
- **New concrete lever: `ui_layout_system` ~4.7 ms on every aditi frame**
  (was ~1.1 ms in the 2026-08-12 capture). Something other than
  `EditableText` still dirties taffy every frame — find it with a `Last`
  system counting per-frame changes of the taffy dirty sources (`Node`,
  `ContentSize`, `ComputedUiRenderTargetInfo`, `Children`), skipping its first
  run (see the `editable-text-per-frame-relayout` finding).
- Other Main costs per frame (whole session, aditi): `collect_visible_cpu_
  culled_entities` 6.3 ms, `mark_3d_meshes_as_changed…` 1.8,
  `build_static_colliders` 1.7, the `check_entities_needing_specialization`
  family ~1 ms each (parallel).

### The `ui_layout_system` cost, chased down (2026-09-25)

The 4.7 ms per aditi frame had two parts, both fixed:

- **Every refresh helper "guarded" its write through a `&mut T` parameter.**
  `ui_text::set_text(text: &mut Text, …)` compared before writing, but its
  callers hold a `Mut<Text>`, and the coercion to `&mut Text` is a `DerefMut`,
  which flags the component changed *before* the comparison runs. So every
  per-frame refresh re-flagged its text (a docked Conversations tab label,
  unchanged all session, re-measured every frame) and dirtied the whole main
  UI tree. `set_text`, `set_disabled_class`, `edit_contents::set_row_text` and
  the two slider `show_value`s now take `&mut Mut<T>`.
- **Any change anywhere re-laid-out the whole screen.** Our Bevy fork now has
  an `IndependentLayout` marker and lays out only dirty layout roots; every
  free floater carries it. The minimap compass (which also had a rounded-size
  feedback loop moving it a fraction of a pixel every frame, now read from the
  unrounded size) costs ~0.1 ms instead of the whole screen.
- Also fixed on the way: bevy_flair flagged every styled component of a
  restyled entity changed (`Node`, `TextFont`…) for a colour animation.

Aditi, visible steady state (p50):

| | before | after |
| --- | --- | --- |
| `ui_layout_system` | 4.75 ms, every frame | 0.41 ms, 75 % of frames (0.55 ms/frame) |
| `PostUpdate` | 18.7 ms | 12.4 ms |
| `Main` | 37.2 ms | 27.2 ms |
| frame | 49.2 ms (≈20 fps) | **36.6 ms (≈27 fps)** |

`Render` (33.5 ms p50) is now the longer thread: the next levers are the
render-world prepare/queue phase and `camera_driver`'s pass encoding.

### What dominates now (aditi, 2026-09-25, after the layout work)

Steady window (1832 frames), per frame: frame p50 36.6 ms, `Main` 27.2,
`Render` 33.5 — **render is the gater**.

| Render-thread cost | ms/frame | note |
| --- | --- | --- |
| main camera `camera_driver` | ~18.5 | Camera 0's 3D + shadow + probe passes |
| ↳ transparent pass `CommandEncoder::finish` | **13.5 mean** | p50 2.1 / frame, **p90 55.7**, single calls up to 188 ms |
| `collect_visible_cpu_culled_entities` | 5.1 | render-world visibility gather |
| `submit_pending_command_buffers` | 4.6 | submit + wgpu maintain |
| shadows (queue + specialize + pass) | ~4.8 | |
| other cameras | ~2.8 | two small camera schedules |
| `vkQueuePresentKHR` | 0.1 | no GPU / vsync wait |

- **The spike source:** wgpu's CPU-side command-buffer processing (validation,
  resource tracking) of the main camera's **transparent** pass. Usually cheap,
  but ~1 frame in 10 it costs 50–190 ms, which is most of the 13.5 ms mean and
  the 104 ms p95. It points at a very large number of individually-bound
  transparent draws (alpha-blended prim faces, name tags …). Next: count
  `Transparent3d` items / draws per frame and what they are; check whether our
  `Transparent3d` re-sort or per-face materials defeat batching, and whether
  the spikes line up with bursts of new bind groups (`Device::create_bind_group`
  147/frame, `BindGroup::drop` max 135 ms) as textures stream in.
- `Main` (no longer gating): `PostUpdate` 12.4 ms of many small systems
  (`build_static_colliders` 1.5, `mark_3d_meshes_as_changed…` 1.5, the
  `check_entities_needing_specialization` family ~1.3 each in parallel,
  bounds / visibility ~1 each), `Update` 10.9 ms; UI layout 0.5 ms/frame.
