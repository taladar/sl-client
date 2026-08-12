# Fully GPU-driven avatar rendering — implementation design

> **Status:** this is the design reference for the GPU-avatar work. The epic
> overview is `viewer-perf-gpu-avatar-crowd`; the work is decomposed into the
> phase tasks `viewer-perf-gpu-avatar-phase0-mesh-dedup`,
> `viewer-perf-gpu-avatar-keystone-skinuniforms-spike`,
> `…-phase1-gpu-fk-palettes`, `…-phase2-gpu-sample-blend`,
> `…-phase3-gpu-picking`, `…-phase4-remove-scaffolding`,
> `…-phase5-lod-polish`. Each phase file links back to the relevant section
> here. Produced by a design pass on 2026-08-12; refine here as
> implementation learns.

Detailed expansion of `roadmap/ideas/viewer-perf-gpu-avatar-crowd.md`.
Target: the dance-club crowd (everyone near, everyone animating), where the
2026-08-12 Tracy critical path shows the frame co-limited main-app (~41 ms) ≈
render-app (~40 ms) plus a fully serial ExtractSchedule (~7 ms, ~90 %
`extract_skins`). The design removes the three CPU stages that scale as
`O(avatars × joints)` — CPU sample+blend (`pose_avatar_skeletons`), transform
propagation of `N × ~200` joint entities, and `extract_skins` — and unlocks
same-mesh instanced draws on the render thread, with GPU picking so no CPU
copy of posed geometry survives for picking's sake.

## 0. Feasibility on the pinned versions (checked, not assumed)

Pinned: **bevy 0.19.0**, **wgpu 29.0.4** (Cargo.lock). What that already
gives us — all verified in the vendored sources:

- **Compute shaders**: core wgpu; the workspace already runs custom passes as
  systems in the `Core3d` schedule (`glow.rs`, `exposure.rs` — the local
  precedent for "Bevy 0.19 is system-based, no render-graph `ViewNode`").
  `Core3dSystems` sets: `Prepass → MainPass → EarlyPostProcess →
  PostProcess` (bevy_core_pipeline `schedule.rs:47`).
- **Skinned-mesh batching already exists**: Bevy 0.19 keeps all joint
  matrices in one persistent buffer (`SkinUniforms.current_buffer`,
  storage-buffer on desktop; `bevy_pbr/src/render/skin.rs`) and routes each
  instance to its palette via `mesh[instance_index].current_skin_index`
  (`skinning.wgsl:37`, `MeshInputUniform.current_skin_index`, mesh.rs:543).
  `NoAutomaticBatching` is only forced on uniform-buffer (WebGL-class)
  platforms (`no_automatic_skin_batching`, skin.rs:502). So on Vulkan,
  **same-mesh + same-bind-group skinned draws batch/instance today** — the
  reason they don't in our scene is that we mint a fresh `Handle<Mesh>` per
  wearer (`build_rigged_submeshes`, objects.rs:5057).
- **Indirect multi-draw**: `GpuPreprocessingMode::Culling` with
  `multi_draw_indirect(_count)` is auto-detected and already active (the
  Tracy trace shows `write_indirect_parameters_buffers`).
- **Bindless materials**: `FaceMaterial` is already declared bindless
  (`face_material.rs:258`, `#[bindless(index_table(range(50..59),
  binding(100)))]`), giving cross-material batching — per-avatar bake
  textures do not split instanced draws.
- **Per-instance u32 tag**: `MeshTag` (extracted into the mesh uniform,
  mesh.rs:1880) — the per-instance ID channel GPU picking needs. Already
  used by the name-tag billboard renderer, on its own entities only.
- **Async readback**: `bevy_render::gpu_readback::{Readback,
  ReadbackComplete}` exists in 0.19.
- **Storage buffers in vertex stage**: required and present on desktop
  (Bevy gates skinned batching on exactly this).

**No version blockers on desktop Vulkan/DX12/Metal.** The one platform hole
is WebGL2/downlevel (no storage buffers, no compute): the design keeps the
current CPU path compiled as the fallback until the final removal phase, and
selects at startup from `RenderDevice` limits (mirroring
`skins_use_uniform_buffers`).

**The load-bearing integration trick**: we do NOT fork Bevy's draw path.
The compute pipeline's final pass writes skin palettes **into
`SkinUniforms.current_buffer` at offsets Bevy allocated**, so
`skinning.wgsl`, batching, indirect draws, shadows, prepass and motion
vectors all keep working unmodified. Details and the ordering proof in §2.4.

---

## 1. GPU data model & buffer layout

### 1.1 The canonical skeleton and indexing scheme

One skeleton XML serves every avatar, so there is **one canonical joint
index space** shared by all GPU data: the `BevySkeleton` order (bones 0..133,
then collision volumes ~26, then the synthetic root, then non-HUD attachment
points ~38) — call it `N_J ≈ 200`, fixed at library load. Everything below
indexes joints by this canonical index; clip joint *names* are resolved to
canonical indices once at clip upload; each mesh asset's skin `joint_names`
are resolved once at mesh-skin upload.

How an instance finds everything (the full indirection chain):

```text
draw instance (Bevy)             ── current_skin_index ──▶ palette slot in
                                                           SkinUniforms buffer
palette slot  ◀─ written by pass D from ─ SkinInstance {
                                            avatar_slot,   // → world matrices
                                            mesh_skin_id,  // → joint_map + IBP
                                            palette_offset // = skin_index
                                          }
avatar_slot   ─▶ AvatarRest[avatar_slot]     (rest skeleton, change-driven)
              ─▶ AvatarFrame[avatar_slot]    (root affine, per frame if moved)
              ─▶ AvatarPlayback[avatar_slot] (active clips, change-driven)
active clip   ─▶ ClipTable[clip_id]          (header → track data, static)
              ─▶ PoseCache[cache_slot]       (per (clip, phase-bucket), transient)
textures      ─▶ unchanged: bindless FaceMaterial per face entity
```

`avatar_slot` is a dense u32 allocated by a CPU-side slot allocator
(free-list) when an avatar rigs, freed on despawn; animesh control avatars
take slots from the same allocator in a later phase. All cross-buffer
references are u32 indices, never pointers.

### 1.2 Static, upload-once-per-asset buffers

**(a) Clip buffer** — one append-only pool (grow-and-copy arena) holding
every decoded `.anim`, uploaded once when `AnimationManager` finishes a
decode. Keyframes are kept **exactly as decoded** (times + values, GPU
binary search), not re-baked to a uniform rate — exactness lets the WGSL
sampler be golden-tested against `sl_anim::sample_motion`, and dance clips
are small (30 joints × ~200 keys × 16 B ≈ 100 KB).

```text
// std430; all offsets are element indices into the pool's typed arrays.
#[repr(C)] struct GpuClipHeader {
    duration: f32, loop_in: f32, loop_out: f32,
    ease_in: f32, ease_out: f32,
    flags: u32,               // bit0 loops
    base_priority: i32,
    track_count: u32, track_offset: u32,
    // joint → track lookup for the gather in pass B:
    // track_of_joint_offset points at N_J u16s (0xFFFF = clip has no track
    // for that joint).
    track_of_joint_offset: u32,
    _pad: [u32; 2],
}
#[repr(C)] struct GpuJointTrack {
    joint: u32,               // canonical index
    priority: i32,            // already resolved (USE_MOTION → base)
    rot_offset: u32, rot_count: u32,
    pos_offset: u32, pos_count: u32,
    flags: u32,               // bit0 pos_is_additive (mPelvis / volume —
                              // resolved at upload from the canonical index)
    _pad: u32,
}
// keys: two typed arrays in the same pool
// rot_keys: array<vec4<f32>> with .w = time? NO — keep time separate for
// clean binary search on a packed f32 array:
//   rot_times: array<f32>, rot_values: array<vec4<f32>>   (xyzw quats)
//   pos_times: array<f32>, pos_values: array<vec4<f32>>   (xyz, w unused)
```

**(b) Mesh-skin buffer** — one entry per **rigged mesh asset** (dedup'd by
`MeshKey` + LOD, alongside the geometry cache), shared by every wearer:

```text
#[repr(C)] struct GpuMeshSkin {
    joint_count: u32,         // K, the mesh's skin joint list length
    joint_map_offset: u32,    // K u32s: canonical joint index per palette slot
    ibp_offset: u32,          // K inverse-bind mat4x4 (or 3x4, see §9)
    flags: u32,               // bit0 lock_scale_if_joint_position
}
```

Joint-position **overrides** are *not* here: they are per-avatar effective
state (`effective_joint_overrides` merges across all worn rigs), so they
fold into the per-avatar rest skeleton below.

### 1.3 Per-avatar change-driven buffers

**(c) Avatar rest buffer** — `AvatarRest[avatar_slot] : [GpuRestJoint; N_J]`.
The CPU composes, per joint, exactly what the head of
`deformed_world_matrices` computes from its inputs (shape `deform` +
`volumes` + `overrides`), and uploads the result — so the GPU FK never needs
to know about sliders, volume morphs, or overrides individually:

```text
#[repr(C)] struct GpuRestJoint {
    rest_pos: [f32; 3], parent: u32,       // canonical parent index, ~0 = root
    rest_rot: [f32; 4],                    // local rest rotation (bones: id)
    local_scale: [f32; 3],                 // deformed scale (or pinned default
                                           //   when override + lock_scale)
    flags: u32,  // bit0 is_volume, bit1 is_pelvis, bit2 has_override
}
// 48 B × 200 × avatar → ~10 KB/avatar; 100 avatars ≈ 1 MB.
```

Re-uploaded (one `write_buffer` of 10 KB) when — and only when — the
avatar's `pose_inputs_generation()` bumps (the existing appearance-change
invalidation counter: shape edit, appearance message, worn-rig override
add/remove, volume-morph toggle). While a user drags a shape slider this is
one small upload per change frame — cheap; no per-frame cost otherwise.

**(d) Playback buffer** — `AvatarPlayback[avatar_slot]`:

```text
const MAX_ACTIVE: usize = 16;              // reference blends 4/joint; 16
                                           // active motions covers AO+gesture
                                           // +typing+dance stacks
#[repr(C)] struct GpuPlayState {
    clip_id: u32,                          // ~0 = empty slot
    start: f32,                            // Time::elapsed_secs at activation
    stopped_at: f32,                       // relative, NaN/-1 = still active
    anim_offset: f32,                      // walk-speed clock skew (P31.14)
    order: u32,                            // recency stamp (truncated u64 ok:
                                           //   only relative order per avatar)
    cache_slot: u32,                       // → PoseCache entry this frame
    _pad: [u32; 2],
}
#[repr(C)] struct GpuAvatarPlayback { slots: [GpuPlayState; MAX_ACTIVE] }
```

Key property: because ease weights and playheads are all functions of
`(now, start, stopped_at, anim_offset)`, and `now` is a per-frame uniform,
this buffer changes **only when the playing set changes** (an
`AvatarAnimation` reconcile, a client locomotion/typing transition) — except
`anim_offset`, which drifts per frame only for a walk-class clip on a
walking avatar (upload just those rows). Idle loops and dances cost zero
upload per frame. `cache_slot` is rewritten by the CPU scheduler each frame
(§2.1) as part of the same small upload — see §2.1 for why that stays CPU.

**(e) Per-frame small uniforms/buffers** — the staged-each-frame structs:

```text
#[repr(C)] struct GpuAvatarFrame {
    root_from_avatar: [ [f32; 4]; 3],       // Bevy-world affine of the avatar
                                           // root (SL→Bevy axis change + place)
    flags: u32,                            // bit0 t_pose (debug freeze)
    idle_seed: u32,                        // per-avatar phase for breathe/sway
    _pad: [u32; 2],
}
// uploaded for avatars whose root anchor Transform changed (the existing
// anchor-moved signal); everyone on login/rebase.

#[repr(C)] struct GpuCorrection {          // sparse CPU adjuster injections
    avatar_slot: u32, joint: u32,
    mode: u32,        // bit0 replace_rot, bit1 replace_pos, bit2 add_pos,
                      // bit3 slerp_rot (weighted toward correction)
    weight: f32,
    rot: [f32; 4], pos: [f32; 3], _pad: f32,
}
// count per frame ≈ (avatars with active look-at/reach/IK/physics) × (few
// joints each) — typically < 64 entries total.

#[repr(C)] struct GpuFrameParams {
    now: f32, idle_now: f32,               // idle_now = 15 Hz-quantized clock
    correction_count: u32, sample_job_count: u32,
}
```

**(f) Working buffers (persistent, GPU-only)** — scratch, never uploaded:

```text
PoseCache   : [PoseCacheEntry; MAX_CACHE]      pass A output / pass B input
  entry     = per clip track t: { rot: vec4, pos: vec4 } (pos.w = channel
              flags), indexed track-major: cache_base + t
LocalPose   : [avatar_slot][N_J] × { rot: vec4, pos: vec4 }
              pass B output / pass C input (pos.w flags: has_rot, has_pos)
JointWorld  : [avatar_slot][N_J] × mat4x4      pass C output (Bevy world
              space, root affine already composed) / pass D + pick input
SkinInstance: [ { avatar_slot, mesh_skin_id, palette_offset } ]  pass D input,
              rebuilt by CPU when the set of visible skinned instances changes
```

`JointWorld` at 100 avatars × 200 × 64 B ≈ 1.3 MB; palettes live in Bevy's
own `SkinUniforms` buffer as today (~7 MB for a heavy crowd — unchanged).

### 1.4 Textures

Nothing new: per-avatar bakes/BOM/clothing stay per-face `FaceMaterial`
entities, whose **bindless** slabs already let instanced draws span
different textures. No texture arrays needed; per-instance material routing
is Bevy's own bindless material index in the mesh uniform. (§3.3 covers the
alpha/BOM specifics.)

---

## 2. The compute pipeline and its Bevy 0.19 scheduling

### 2.1 CPU-side frame prep (main world, `Update` — replaces the heavy fold)

A slim `schedule_gpu_avatars` system (successor of
`drive_avatar_skeletons`'s bookkeeping half) does, per frame:

1. Reconcile `AvatarAnimation` events into `AnimationPlayback` exactly as
   today (`reconcile_playing`, `retain_active`, walk-speed clocks). This is
   event-driven and tiny.
2. Build the frame's **sample-job list**: the set of distinct
   `(clip_id, phase_bucket)` across all avatars' active slots.
   `phase_bucket = round(anim_elapsed × 30)` for looping clips of avatars
   beyond a "sync distance" knob; exact per-avatar phase (bucket = unique)
   for near/own avatars so nothing visibly snaps. Assign each job a
   `cache_slot`; write it into the affected avatars' `GpuPlayState`s.
   This dedup **is** the animation-data instancing: 40 synced dancers on one
   dance = 1 sample job. Kept on CPU deliberately: it is O(active clips)
   hashing/allocation — pennies on CPU, awkward (hash tables, atomics) on
   GPU. Justified under the user's test: CPU is better suited AND cheap.
3. Upload deltas: changed playback rows, changed `GpuAvatarFrame` rows,
   the correction list (§5), the sample-job list, `GpuFrameParams`.
4. Run the CPU retentions of §5 (socket FK, adjusters) — these *read* the
   mini-pose, not the GPU.

No `deformed_world_matrices`, no `write_joint_globals`, no `PoseGate`
needed: the GPU costs the same whether a pose changed or not, and the whole
class of change-detection/propagation-stomp bugs
(`sl-client-bevy-change-detection-gotchas`,
`sl-client-pose-driver-orphans-joint-children`) exits with the joint
entities.

### 2.2 The four compute passes

All WGSL below is sketch-level but names the real responsibilities.

**Pass A — clip sample (dedup'd).** Dispatch: one workgroup per sample job,
64 threads over the clip's tracks.

```wgsl
// thread (job, t): sample track t of job.clip at job's phase.
let clip = clips[job.clip_id];
let track = tracks[clip.track_offset + t];
// loop wrap (loop_in/loop_out), then binary search rot_times /
// pos_times (exact port of sl_anim::sample interpolation: nlerp
// short-arc for rotations, lerp for positions).
pose_cache[job.cache_base + t] = SampledTrack(rot, pos, flags);
```

**Pass B — per-joint priority/ease blend + idle + corrections.** Dispatch:
one thread per `(avatar, joint)` — `avatars × N_J` threads (100 avatars =
20 k threads; trivial).

```wgsl
// gather ≤ MAX_ACTIVE contributions for this joint
var contribs: array<Contribution, MAX_ACTIVE>;
for (var s = 0u; s < MAX_ACTIVE; s++) {
    let play = playback[avatar].slots[s];
    if (play.clip_id == EMPTY) { continue; }
    let track = track_of_joint(play.clip_id, joint);   // u16 lookup, §1.2
    if (track == NO_TRACK) { continue; }
    let w = pose_weight(clip, params.now - play.start, play.stopped_at);
    // exact port of Motion::pose_weight (cubic ease in/out, wall time)
    if (w <= 0.0) { continue; }
    push(contribs, Contribution(track_priority, play.order, w,
                                pose_cache[play.cache_base + track]));
}
// exact port of blend_joint: sort by (priority desc, order desc), cap 4,
// fold highest-first with the running weight budget
// (new_sum = min(1, w + sum); nlerp(sum/new_sum, incoming, accumulated)).
var local = blend(contribs);
// idle adjusters (breathe/sway): pure f(idle_now, idle_seed, joint) —
// port of procedural::apply_idle_adjustments, composed like today.
local = apply_idle(local, joint, frame[avatar]);
// sparse CPU corrections (look-at / reach / IK / physics), §5:
// corrections are pre-sorted by (avatar, joint); binary-search my range.
local = apply_corrections(local, avatar, joint);
local_pose[avatar * N_J + joint] = local;
```

Sorting 4-of-16 contributions per thread is a fixed small insertion sort —
no shared memory needed.

**Pass C — hierarchical FK (the SL recurrence) + root compose.** The SL
recurrence is order-dependent (parent before child). Two strategies:

- **v1 (recommended): one thread per avatar**, serial loop over the N_J
  joints in canonical order (parents precede children by construction).
  ~200 iterations of a few fma each; 100 avatars = 100 threads. Occupancy
  is terrible but absolute cost is microseconds — measure before
  complicating.
- **v2 (if v1 ever shows in a GPU capture): workgroup per avatar**,
  level-ordered: precompute level lists (skeleton depth ≈ 12–16); loop
  levels with `workgroupBarrier()`, threads cover joints in the level.

```wgsl
// exact port of BevySkeleton::deformed_world_matrices' inner loop:
//   scale     = rest.local_scale                     (pre-composed on CPU)
//   local_rot = pose.has_rot ? pose.rot : rest.rot
//   base_pos  = rest.rest_pos                        (overrides pre-folded)
//   pos       = pose.has_pos
//               ? (is_volume || is_pelvis ? base_pos + pose.pos
//                  : has_override ? base_pos : pose.pos)
//               : base_pos
//   child offset scaled by PARENT's local scale, rotated into parent frame;
//   own scale enters only the final matrix (never inherited).
// then: world[j] = root_from_avatar * TRS(pos_w, rot_w, scale)
```

One semantic note baked into (c)/(e): today `override_pos.is_some()` wins
over an absolute position key *per joint*; the rest buffer carries
`has_override` per joint so the GPU reproduces that exactly.

T-pose debug freeze: `frame.flags.t_pose` makes pass B output "no channels"
so pass C yields the shaped rest — preserving the `SL_VIEWER_TPOSE` A/B
harness.

**Pass D — skin palettes.** Dispatch: one thread per palette entry, i.e.
`Σ_instances K_instance` (≈ 100 avatars × ~10 skins × ~80 joints = 80 k).

```wgsl
let inst = skin_instances[instance_of(gid)];   // prefix-sum lookup or
                                               // (instance, k) 2D dispatch
let skin = mesh_skins[inst.mesh_skin_id];
let cj   = joint_map[skin.joint_map_offset + k];
let m    = joint_world[inst.avatar_slot * N_J + cj]
         * ibp[skin.ibp_offset + k];
bevy_skin_palette[inst.palette_offset + k] = m;   // SkinUniforms buffer!
```

### 2.3 Scheduling in Bevy 0.19 (system-based renderer)

Following the `glow.rs` / `exposure.rs` precedent exactly:

- `RenderStartup`: create the four compute pipelines + persistent buffers
  (`GpuResourceAppExt` pattern).
- `ExtractSchedule`: a light `extract_gpu_avatar_frame` copies the CPU-side
  staging structs (job list, changed rows, corrections, params) — plain
  `Vec` memcpys, byte-sized; this is what *replaces* the 5–7 ms
  `extract_skins` share.
- `Render` schedule, `RenderSystems::PrepareResources`: write the staged
  deltas into the GPU buffers (`RenderQueue::write_buffer`); resolve each
  registered skin instance's `palette_offset` from
  `SkinUniforms::skin_index(main_entity)` (public API) and rebuild the
  `SkinInstance` buffer when the instance set or offsets changed.
- `Render` schedule, `RenderSystems::PrepareBindGroups`: (re)build the pass
  bind groups. The pass-D bind group binds `SkinUniforms.current_buffer`
  (pub field) as `storage, read_write` — **rebuilt every frame** because
  `prepare_skins` swaps current/prev buffers each frame.
- `Core3d` schedule: one system `run_avatar_compute` encoding passes A→D in
  a single `ComputePass`, ordered `.in_set(Core3dSystems::Prepass)` but
  `.before(prepass render systems)` — i.e. first in `Prepass` — so palettes
  are final before any depth/shadow/main pass samples them. (Shadow passes:
  verify where the shadow-map pass encodes in 0.19; if it encodes outside
  `Core3d`, order the compute in `Render` before the pass-encoding set
  instead — same command-stream position, different label. This is a
  named implementation check, not a design risk.)

### 2.4 Why writing into `SkinUniforms.current_buffer` is sound

Frame order (all on the render app, single command stream):

1. `prepare_skins` swaps `current`↔`prev`, then `queue.write_buffer`s the
   CPU staging (rest-pose junk for our frozen-joint skins; live data for any
   remaining CPU skins e.g. animesh pre-migration). wgpu guarantees queue
   writes land **before subsequently submitted command buffers**.
2. Our compute (encoded in `Core3d`, submitted after) overwrites our
   instances' palette ranges with posed matrices.
3. Draw passes read the buffer.

Motion vectors / TAA: `prev` holds last frame's compute-written buffer
(the swap happens before the staging write, and the staging write only
touches `current`) — so `skin_prev_model` reads last frame's *posed*
palettes. Correct.

Two gotchas, both handled by the migration trick in §7.2:

- `prepare_skins` early-returns (no swap!) when the staging buffer is
  empty. Our registered skins keep the staging non-empty, so the swap
  always runs.
- Buffer growth reallocates both buffers; offsets can move on
  add/remove. We re-resolve `palette_offset`s every frame from
  `skin_index()` (cheap: one hash lookup per skinned instance).

The full-staging `write_buffer` re-uploading rest junk for our slots each
frame is wasted bandwidth (~ a few MB/frame at crowd scale). Acceptable for
the transition; the §7 endgame removes those bytes from the staging buffer
entirely (frozen dummy-joint skins stage `K` matrices once and are never
re-written — the buffer still uploads them; if measurement shows it matters,
upstream a `SkinUniforms` "external palette" flag or maintain our own palette
buffer + a forked `skinning.wgsl` binding as the last resort — see §9).

---

## 3. Instancing

### 3.1 What instances with what

Grouping key for a batched/indirect draw in Bevy 0.19: (pipeline, mesh
asset, bind groups). Per-instance data already routed through the mesh
uniform: transform, `current_skin_index`, `MeshTag`, bindless material
index. Therefore:

- **Rigged mesh assets (bodies, heads, hair, clothing)** — the crowd case:
  N wearers of one body = one instanced draw per (submesh × alpha mode),
  each instance with its own palette offset and bindless material. This is
  *per mesh asset, not per avatar* by construction: each worn submesh is
  its own instance in its asset's batch.
- **System (morphable) body parts**: per-avatar `Mesh` assets (shape
  sliders morph vertices CPU-side today) — they do not instance across
  avatars and that is fine (legacy minority); they still ride the same GPU
  pose/palette path.
- **Rigid attachments / prims**: already covered by the geometry cache;
  unchanged.

### 3.2 Unlocking it: share the mesh assets (today's gap)

`build_rigged_submeshes` (objects.rs:5057) does
`meshes.add(to_bevy_rigged_mesh(submesh))` per wearer, and mints a fresh
`SkinnedMeshInverseBindposes` per wearer (objects.rs:5043) — so two Maitreya
wearers never share a `Mesh` and never batch. Fix (Phase 0, standalone win
even on the CPU pose path):

- Extend `GeometryCache` with a rigged-submesh slot keyed
  `(MeshKey, lod, submesh_index)` (weak `AssetId` + revive semantics,
  exactly like the prim slots).
- Cache `SkinnedMeshInverseBindposes` and the future `GpuMeshSkin` by
  `(MeshKey, lod)`.
- Per-wearer differences (BOM tint/UV, textures) stay in the per-entity
  `FaceMaterial` — bindless keeps them batchable.

Then verify in a two-wearer OpenSim scene that RenderDoc/Tracy shows one
instanced draw per submesh. (Watch: `NoFrustumCulling` on rigged submeshes
keeps them all in every batch — fine, they're avatars we want drawn; §8 adds
GPU bounds later.)

### 3.3 Alpha / BOM / transparency

- **Opaque + alpha-mask faces** (most body/clothing area): batch freely in
  the binned opaque/alpha-mask phases. BOM per-face tint/hide/blend already
  lands in per-face material data (bindless).
- **Alpha-blend faces** (hair, lashes, sheer layers): `Transparent3d` is a
  sorted phase; Bevy batches adjacent same-key items, so interleaved depths
  split batches. Correctness unchanged; instancing win partial. This is the
  same trade the whole engine makes — do nothing special. If a club scene
  shows transparency dominating, the existing `Transparent3d` re-sort
  machinery (`sl-client-transparent-phase-resort`) is the hook for a
  per-asset secondary sort key experiment. OIT is out of scope (Bevy 0.19's
  OIT layer is not enabled in this viewer; revisit only if measurements
  demand).
- **Glow/emissive avatar faces**: unchanged (material-level).

### 3.4 Animation-data instancing

Already in the data model: shared `ClipBuffer` (one copy per asset,
whatever the avatar count) + the `(clip, phase_bucket)` PoseCache dedup
(§2.1). Synced club dancers collapse to one sample job; unsynced dancers
collapse to ≤ 30 jobs/s per clip. Blend (pass B) stays per-avatar because
ease weights, activation order and the rest skeleton differ — that is the
correct sharing boundary (same conclusion as the pose-cache roadmap item:
same dance ≠ same skin matrices unless same shape).

---

## 4. One-time / change-driven uploads and invalidation triggers

| Data | Upload | Trigger / invalidation |
| --- | --- | --- |
| Clip tracks (`GpuClipHeader`+keys) | once per decoded `.anim` | `AnimationManager` decode completion; freed by LRU eviction if the arena ever matters (clips are tiny) |
| Mesh skin (`GpuMeshSkin`, joint map, IBPs) | once per rigged mesh asset | mesh decode; freed with the geometry-cache entry |
| Mesh vertices/weights | once per asset (Bevy `Mesh`) | already the case; §3.2 makes them shared |
| Avatar rest skeleton (`GpuRestJoint[N_J]`) | on change | `pose_inputs_generation()` bump (shape/appearance edit, joint-override add/remove, volume morphs) — the existing hook, per-avatar granularity via a per-avatar generation copy |
| Playback rows | on change | `reconcile_playing` outcomes; walk-clock rows for currently-walking avatars only |
| `GpuAvatarFrame` (root affine) | on change | root anchor `Transform` changed (read the anchor `Transform`, not `GlobalTransform` — the one-frame-lag memory note) |
| Corrections | per frame, sparse | only avatars with an active adjuster; empty most frames for most avatars |
| Sample-job list + `cache_slot`s | per frame, tiny | derived from playback sets; O(active clips) |
| `SkinInstance` table | on change | skinned instance spawned/despawned/re-offset (skin_index moved) |
| Textures / bakes | unchanged | existing bake/texture pipelines |

Shape editing while the wearer watches (the "while not editing shape"
carve-out from the goal): each slider tick bumps `pose_inputs_generation`,
re-composes 10 KB on CPU, uploads it — well under a millisecond; live
preview stays smooth and there is still zero steady-state cost.

---

## 5. The CPU/GPU boundary — every retention justified

Test applied: *CPU only if genuinely better-suited AND cheap.*

**Stays CPU (all bounded, none O(avatars × joints × fps)):**

1. **Playback reconcile + asset fetch/decode + clip upload staging.**
   Network/event driven, hash-map logic, IO. O(events). Obviously CPU.
2. **Sample-job scheduling / cache-slot + avatar-slot allocation** (§2.1).
   O(active clips) hashing and free-lists; GPU would need device-side hash
   tables and atomics for zero win. CPU-better and ~µs.
3. **IK / look-at / reach / locomotion / body-physics adjusters.**
   World-state-dependent, iterative, few joints, active on few avatars
   (mostly the own avatar + lookers). They run against a **CPU mini-pose**:
   `resolve_pose` restricted to the union of joints the active adjusters
   need (leg chains, neck/head/eyes, arms — ~25 of 200), sampled from the
   same `Motion`s, plus the SL recurrence over just those chains (a
   `deformed_world_chain(joints_subset)` variant). Output: `GpuCorrection`
   entries (replace-rot for look-at/IK results, additive-pos for physics
   volume displacements — matching today's pose channels). No GPU readback:
   a readback would add 1–2 frames of latency inside a *servo* (foot IK
   already fights oscillation — the near-singular-leg memory note), and the
   probe/servo state machines are branchy CPU code today. Cost: ~25 joints
   × (few active avatars) — pennies. The ground probe keeps its pre-IK
   ankle targets from the same mini-FK (it must NOT read the posed result —
   preserved invariant).
4. **Socket joints** (attachment points in use, name-tag anchor, camera
   focus joint, eye/head for the own-avatar camera): short-chain CPU FK
   from the same mini-pose, writing the socket entity's **`Transform`
   relative to the avatar root** so ordinary change-gated propagation
   places the rigid-attachment subtree — which *deletes* the
   `pose_attachment_nodes` hand re-propagation and the orphaned-children
   bug class entirely. Per avatar: sockets actually carrying something
   (typically 2–10) × chain depth ≤ 12, and only for avatars wearing rigid
   attachments. GPU-readback alternative rejected: visible 1–2-frame lag on
   a hand-held object is worse than the milli-cost. Determinism note: CPU
   mini-FK and GPU FK run the same algorithm on the same inputs; f32
   ordering differences are sub-mm — a rigid ring on a rigged glove will
   not visibly separate. Golden tests pin both to the same reference
   values within 1e-4.
5. **Legacy system-body morph baking** (shape sliders → base mesh vertices;
   blink/physics `*_Driven` morphs). Change-driven (appearance edit, blink
   edges at a few Hz), per-avatar meshes, existing code. Moving these to
   GPU morph targets is a separate follow-up with its own payoff profile —
   explicitly out of scope, and it does not violate the per-frame test
   (blinks are event-ish, not per-frame-per-joint).
6. **Pick scheduling + ID registry + readback mapping** (§6): O(1) per
   frame.

**Leaves the CPU/ECS entirely:**

- Per-joint entities (~200/avatar) and their `Transform`/`GlobalTransform`
  — replaced by `avatar_slot` buffer rows. Only the avatar root anchor, the
  body-part/face entities, socket entities in use, and name-tag entities
  remain in ECS.
- `pose_avatar_skeletons`' full solves (the 2× `deformed_world_matrices`),
  `write_joint_globals`, `pose_attachment_nodes`, `PoseGate`.
- `extract_skins`' avatar share (the ~7 ms serial segment).
- `fit_avatar_pick_colliders` + `avatar_pick.rs`' on-demand CPU skinning
  (superseded by §6), and the hover `MeshRayCast` world casts.

---

## 6. GPU picking

### 6.1 Architecture: cursor-cropped ID pass + async readback

A dedicated **pick view**: an offscreen camera whose projection is the main
camera's frustum cropped to a small square around the cursor (e.g. 9×9 px
at full-res scale), rendering two attachments:

- `R32Uint` **ID target**: fragment writes the instance's pick ID.
- `Depth32Float` depth target: for the world-space hit point.

Cropping via projection (not scissor) means Bevy's frustum culling reduces
the candidate set to the handful of entities under the cursor — vertex cost
collapses too, not just fill.

**Pick IDs** ride `MeshTag` (already per-instance in the mesh uniform, so
it survives batching/instancing). A `PickRegistry` resource allocates:

```text
tag = class:4 bits | index:28 bits
classes: 0 unpickable, 1 avatar submesh (index → AgentKey slot),
         2 object face (index → ScopedObjectId+face slot), 3 terrain,
         4 water, 5 reserved (name tags stay on the CPU rect test — their
         MeshTag is already the atlas channel, and the 2D test is exact
         and cheap)
```

Assigned at spawn where faces/parts spawn (`build_rigged_submeshes`,
`spawn_body_part`, prim face spawn, terrain), freed on despawn. HUD is
excluded from the pick view by `RenderLayers` and keeps `hud_pick.rs`
(orthographic 2D; a follow-up can move it onto a second tiny pick view if
the CPU test ever shows up in traces).

**Pipeline**: one custom `PickPhase` per pick view with exactly two shader
variants — static and skinned — sharing Bevy's mesh vertex layout and
`skinning.wgsl` (so a GPU-posed avatar is picked **exactly where it is
drawn**, palettes included; zero CPU pose duplication). Fragment: write
`mesh[instance].tag`; for alpha-masked materials sample base color via the
material bind group and discard under cutoff; alpha-blend faces are treated
as pickable-opaque above a low alpha floor in v1 (reference-viewer-like;
refine later if hair picking annoys). Implementation shape: a lightweight
queue system that walks the main view's visible entities intersecting the
pick frustum and emits unbatched draws — at ≤ a dozen survivors, batching
is irrelevant, which keeps the custom phase small (no indirect plumbing).

**Readback**: `Readback::texture` on both targets;
`ReadbackComplete` arrives 1–2 frames later. A small ring buffer keyed by
frame index stores the pick camera's `view_from_world`/`clip_from_view` so
the unproject uses the matrices of the *submitting* frame:

```text
world_hit = inverse(clip_from_world[frame]) * ndc(px, py, depth)
```

### 6.2 Consumers and latency budget

- **Hover tooltip**: dwell-gated already; 1–2 frame latency is invisible.
  Pick view enabled at ~15 Hz while the cursor rests on world content —
  the entire `update_hover_tooltip` `MeshRayCast` cost (5.9 ms mean under
  active pointer) is deleted; the occlusion checks (UI hover map) stay as
  they are (already cheap, non-raycast) and the HUD-occlusion raycast is
  replaced by the HUD exclusion + `hud_pick` rect logic.
- **Click select / right-click menus**: request on press, resolve on the
  readback (next frame). Industry-standard; the reference's own pick is
  not same-frame either once you count its render sync.
- **Land pick / double-click teleport / distance**: class 3 + depth
  unproject gives the terrain hit point without the terrain raycast.
- **Box/multi-select (optional, later)**: enlarge the crop to the drag
  rect at reduced resolution and collect unique IDs from the readback.
- **Avatar picking**: class 1 replaces `avatar_pick.rs` (the on-demand CPU
  skinning) and the pick-collider broad phase; morph/physics offsets are
  now *included* (fixes the documented centimetre error the CPU pick had).
  During Phases 0–2 (before the ID pass lands) the existing CPU pick keeps
  working because joint entities still exist; §7 removes them only after
  picking has migrated.

### 6.3 Correctness with GPU-posed geometry

The pick pass runs **after** pass D in the same command stream, reading the
same palette buffer the main passes read — by construction the ID buffer
shows pixels exactly where the visible pass puts them. There is no second
source of truth to drift.

---

## 7. Migration & integration plan (each phase landable + verifiable)

**Phase 0 — instancing unlock + baselines** (no behaviour change)

- Rigged submesh `Mesh` + IBP dedup through `GeometryCache` (§3.2).
- Pick-ID `MeshTag` assignment + `PickRegistry` (inert data).
- Tracy + RenderDoc baseline on a scripted N-avatar OpenSim crowd scene
  (same body asset, same dance) and on aditi.
- Verify: one instanced draw per (submesh, alpha-mode) for same-body
  wearers; `extract_skins` unchanged; screenshots identical.

**Phase 1 — GPU FK + palettes (CPU pose kept)** — kills the serial extract

- Land buffers (§1.2 c/e/f minus clips), passes C+D, the `Core3d` compute
  system, the `SkinUniforms` write-in integration (§2.4).
- CPU still runs `resolve_pose` + adjusters; instead of
  `write_joint_globals`, upload the blended `LocalPose` rows
  (N_J × 32 B × posed avatars — a fraction of extract_skins' matrices) and
  skip passes A/B.
- Joint entities stay spawned but **frozen** (never written) — Bevy's
  `extract_skins` sees no `Changed<GlobalTransform>` and the change-driven
  cost collapses; palettes come from pass D. Socket FK (§5.4) lands here,
  since rigid attachments can no longer read posed joint globals.
- Feature flag `SL_VIEWER_GPU_AVATARS` (default off until verified),
  runtime capability check; CPU path fully intact underneath.
- Verify: screenshot A/B GPU vs CPU path (T-pose harness + animated), Tracy
  A/B: ExtractSchedule median → `extract_lights`-level; PostUpdate
  propagation drop; attachments/name tags/camera all tracking.

**Phase 2 — GPU sample + blend** — kills the main-thread fold

- Clip upload on decode; playback/сorrection buffers; passes A+B; CPU
  `resolve_pose` demoted to the adjuster mini-pose (§5.3).
- Pose-cache dedup + phase buckets; idle adjusters ported into pass B.
- Verify: WGSL golden tests (§9.2); dance-club scene A/B
  (`pose_avatar_skeletons` successor ≈ scheduling-only); long-run soak for
  playback-clock drift (walk speed, loop wrap).

**Phase 3 — GPU picking** (parallelizable with Phase 2 after Phase 0)

- Pick view, ID+depth targets, two pick pipelines, readback + registry
  resolution; port hover tooltip, click/right-click, land pick.
- Delete: `avatar_pick.rs` CPU skinning, pick-collider fitting
  (`fit_avatar_pick_colliders`), world `MeshRayCast` casts in
  `hover_tooltip`/`object_picking`/`land_menu` (keep `MeshRayCast` for any
  non-cursor consumers, e.g. edit-tool axis rays, until separately
  migrated).
- Verify: pick parity suite (§9.2), `update_hover_tooltip` cost → ~0,
  latency feel-check for click select.

**Phase 4 — remove the scaffolding** — delete the transitional CPU path

- Replace per-avatar joint-entity spawning with slot registration; avatar
  skins register in `SkinUniforms` via the **frozen dummy-joint trick**
  (a `SkinnedMesh` whose `joints` are K copies of one shared inert entity —
  keeps Bevy's allocator, `current_skin_index` plumbing, and the every-frame
  buffer swap alive through public API only), or — preferably, attempted
  first — an upstream Bevy PR adding an "externally-written skin" marker so
  the staging bytes disappear too.
- Delete `pose_avatar_skeletons`, `write_joint_globals`,
  `pose_attachment_nodes`, `PoseGate`, the joint-entity spawn path, the
  full-skeleton `deformed_world_matrices` per-frame calls (the function
  stays for rest-pose/one-shot uses: spawn placement, body metrics).
- Animesh control avatars migrate onto avatar slots here (same machinery,
  `ObjectPlayingAnimation` source).
- Remove `SL_VIEWER_GPU_AVATARS` default-off; the flag flips to a
  fallback selector for downlevel platforms.

**Phase 5 — scalability & polish** (§8): bone-count LOD, budgeted sample
rates, GPU-computed skinned bounds to retire `NoFrustumCulling`, optional
box-select, HUD pick view.

Throughout: attachments and picking never break — Phase 1 moves sockets
before joints freeze; Phase 3 replaces picking before Phase 4 removes the
CPU pick's inputs.

---

## 8. LOD & scalability hooks (weak GPUs degrade gracefully)

All knobs live in the CPU scheduler (§2.1) — the GPU passes are
data-driven, so degradation = feeding them less:

- **Phase-bucket coarsening (temporal LOD)**: per-avatar sample rate by
  screen-space size × recency (the `viewer-perf-animation-lod-pose-cache`
  policy): near avatars exact-phase, far/occluded ones bucketed at 15→10 Hz.
  Palettes persist between updates (skip the avatar's B/C/D rows via a
  dirty list), so a skipped avatar holds its last pose — the buffers'
  persistence gives the "budget pose update" for free. NOTE the probe-cadence
  memory lesson: throttle *pose recompute*, never render-resource cadence.
- **Bone-count LOD**: a reduced canonical level list (drop face/finger/wing
  joints) selected per avatar; pass B/C iterate the reduced list, pass C
  writes parent-chain results for skipped joints (inherit parent world) so
  palettes stay valid. Needs no weight remap (weights reference canonical
  indices whose matrices are simply less fresh) — cheaper than the CPU
  variant sketched in the roadmap item.
- **MAX_ACTIVE clamp / priority floor**: crowd-stress option to blend only
  the top-k motions for background avatars.
- **Impostors stay deferred** (`viewer-avatar-impostors-billboard`): this
  design keeps full geometry viable much further out; impostors remain the
  opt-in extreme-count fallback, unchanged.

---

## 9. Risks, unknowns, and the testing/verification strategy

### 9.1 Risks & unknowns (with the experiment that resolves each)

1. **`SkinUniforms` write-in ordering** (§2.4) — the design's keystone.
   *Experiment (do first, Phase 1 spike)*: a toy branch that binds
   `current_buffer` in a compute pass writing a constant palette for one
   skinned mesh; verify draw output + motion vectors + the swap-with-empty-
   staging edge. If bind-usage validation rejects mixing (uniform-bound on
   some platform path), fallback: our own palette buffer pair + a forked
   `skinning.wgsl` via a custom `MaterialExtension` vertex shader on avatar
   materials — more code, no conceptual change. This fork is also the
   escape hatch if the staging-bandwidth waste measures badly.
2. **Bevy 0.19 custom-phase friction for the pick pass** (drawing with
   material bind groups outside the standard phases). *Experiment*: spike
   the static-mesh pick pipeline on prims only; if per-material alpha-mask
   access is painful, v1 ships opaque-only cutout handling (alpha-masked
   faces picked as opaque) and iterates.
3. **Priority/ease blend fidelity** — SL quirks (recency ties, the 4-slot
   cap, weight-budget fold, USE_MOTION priorities, walk-clock skew) are
   behaviour users notice. Resolved by golden tests (below), not by hope.
4. **Adjuster parity** — corrections computed against the CPU mini-pose
   while the GPU blends the full pose: the mini-pose must include every
   motion touching the adjuster chains (it samples the same playing set, so
   it does by construction; the risk is a missed joint in the chain union).
   Diagnostic: `SL_VIEWER_GPU_AVATARS_VALIDATE=1` reads back `LocalPose`
   for one avatar and diffs against a CPU reference each second.
5. **Precision**: world-space palettes at region coords up to ~256 m (plus
   origin-rebase already in place) — same f32 envelope as today; keep
   root-relative math in pass C and compose the root affine last (as
   specified) so limbs never subtract large numbers.
6. **Morph interplay**: collision-volume physics displacements arrive as
   corrections (additive-pos, matching today's pose channel); system-body
   vertex morphs stay CPU (§5.5). Risk is only double-application — the
   rest buffer must NOT fold physics volumes (it folds shape volumes only);
   review checklist item.
7. **Buffer growth/fragmentation**: avatar slots and cache slots are dense
   free-lists; clip arena grows-and-copies. Bounded by design; add F3
   counters.
8. **Frozen-joint `extract_skins` residue** (Phases 1–3): per-skin per-joint
   `changed_transforms.get` misses — measure; expected ≤ 0.3 ms at 100
   avatars, removed in Phase 4.
9. **Two-camera cost for picking** (visibility/queue for the pick view):
   mitigated by the tiny frustum + 15 Hz duty cycle; measure in Phase 3 A/B
   with pointer resting on dense content.
10. **Shared-worker contention**: the compute passes add render-thread
    encode time (~0.1 ms); the win is main-thread + serial-extract time.
    The co-limited framing says both must drop — Phase 1 (extract + main)
    and Phase 0 (render draws) each target one side; verify the *frame*
    median, not one thread (the verify-before-claiming-fixed rule).

### 9.2 Testing & verification

- **Golden unit tests (CPU vs WGSL)**: run each compute pass headless (the
  existing `render_test.rs` proves the harness pattern) on fixture data and
  compare against the Rust reference implementations:
  - pass A vs `sample_motion` (loop wrap, binary search edges, quat nlerp);
  - pass B vs `blend_joint` + `pose_weight` (priority ties, recency, 4-cap,
    weight budget, zero-weight skip, ease in/out cubic);
  - pass C vs `deformed_world_matrices` (pelvis-additive, volume-additive,
    absolute-position replace, override-wins, lock_scale, parent-local-
    scale-scales-child-offset — one fixture per branch);
  - pass D vs the R13-validated `Σ wᵢ (world · ibp)` formula.
  Tolerance 1e-4; exactness assertions (bit-equal) where the CPU port is
  algorithmically identical.
- **Buffer-packing tests**: Rust `#[repr(C)]` structs asserted against the
  WGSL layouts (offset/size const asserts; encase or manual).
- **Screenshot compares**: the headless debug-camera harness
  (`sl-client-viewer-debug-camera`) + `SL_VIEWER_TPOSE` for rest-pose
  A/B, plus animated A/B with a fixed-seed clip at a fixed playhead
  (deterministic by construction: `now` is injectable).
- **Pick parity suite**: for a grid of cursor points over a posed avatar +
  overlapping prim + terrain scene, compare ID-buffer results against the
  CPU `avatar_pick`/raycast answers before those are deleted (allowing the
  documented CPU-pick morph error as the expected diff direction).
- **Tracy A/B (the acceptance test)**: scripted OpenSim crowd (N same-body
  dancers; the conformance harness can drive N logins) and an aditi club
  visit, before/after each phase, window visible; report frame median,
  `ExtractSchedule` median, Main/Render thread medians, and draw-call
  counts. Success criterion for the whole arc: the dance-club frame is no
  longer co-limited by avatar work — `extract_skins` ≈ 0, pose fold ≈
  scheduling-only, one instanced draw per shared submesh.
- **Soak**: 30-min AO/dance run watching for playback drift, slot leaks
  (F3 counters), readback backlog.

---

## Appendix A — What is kept vs replaced (at a glance)

| Today | End state |
| --- | --- |
| `drive_avatar_skeletons` sample+blend | pass A/B (GPU); CPU mini-pose for adjuster chains only |
| `pose_avatar_skeletons` 2× full solves + folds | pass C (GPU) + CPU corrections buffer |
| `write_joint_globals` → ~200 joint entities | `JointWorld` buffer; ~2–10 socket entities per avatar with attachments |
| Bevy transform propagation of joints | gone (sockets ride normal propagation as root children) |
| `extract_skins` (5–7 ms serial) | `extract_gpu_avatar_frame` (byte-sized deltas) |
| `prepare_skins` CPU staging for avatars | pass D writes palettes in place |
| `skinning.wgsl` vertex LBS | **kept unchanged** (required for instancing — compute-skinned posed vertex buffers would be per-avatar and un-instance the draws; no consumer needs posed vertices: picking renders, sockets are CPU-FK) |
| Per-wearer `Mesh` clones | shared assets → instanced/indirect draws (Bevy batching, kept) |
| `avatar_pick.rs` CPU skinning, pick colliders, hover `MeshRayCast` | ID-buffer pick + readback |
| `PoseGate` / idle 15 Hz wakes | gone; GPU cost is flat and tiny |

## Appendix B — Rough GPU budget (100-avatar club, desktop)

- Pass A: ≤ ~50 jobs × ≤ 187 tracks ≈ 10 k threads, trivial ALU.
- Pass B: 100 × 200 = 20 k threads × ≤ 16 gathers.
- Pass C: 100 serial-FK threads × ~200 iterations (v1).
- Pass D: ~80 k threads × one 4×4 multiply.
- Buffers: rest 1 MB, world 1.3 MB, palettes ≈ today's SkinUniforms, clips
  ≈ a few MB total, per-frame uploads ≈ tens of KB.

Estimated total ≪ 0.5 ms GPU — noise against a 40 ms frame; the payoff is
the ~7 ms serial extract, the main-thread fold + propagation, and the
render-thread draw collapse, per the co-limited critical path.
