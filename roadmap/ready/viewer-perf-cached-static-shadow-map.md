---
id: viewer-perf-cached-static-shadow-map
title: Cache the static sun-shadow map; re-render only dynamic casters
topic: viewer
status: ready
origin: off-CPU/critical-path profiling design discussion (2026-08-16/17) — what
  can we retain instead of recomputing per frame
refs:
  - viewer-perf-shadow-cull-change-driven
  - viewer-p24-1-sun-moon-shadow-maps
  - viewer-perf-steady-state-46fps-ceiling
  - viewer-perf-probe-capture-shadows
  - viewer-perf-render-app-bound-frame
  - viewer-perf-per-object-face-merge-entity-count
---

Context: [context/viewer.md](../context/viewer.md).

## Motivation

Profiling (2026-08-16, aditi dense scene) showed the frame is
**latency-bound on a serial critical path**, not throughput-bound: the Compute
pool is underutilised (<=3 of ~15 workers ~90 % of the time) and the machine is
~90 % idle, so the lever is *eliding redundant per-frame recompute*, not adding
threads (see [[viewer-perf-render-app-bound-frame]] and the perf-offcpu memory).

Shadow work (`queue_shadows` + `specialize_shadows` + the shadow passes) is
consistently among the top render-thread systems (~3-4 ms+ of the render leg) —
this is an established cost, no further measurement gate. Bevy has no retained
render phases yet, so it re-renders the sun shadow map from scratch every frame
even when nothing in it changed. In SL almost every shadow caster is **static**
(buildings, terrain, static mesh/sculpt/prims); the things that actually move
are a small, knowable set. So the sun shadow map is mostly identical frame to
frame, and re-rendering the static casters every frame is wasted work.

The idea: render the static casters into a **cached** shadow map, and each frame
render only the **dynamic** casters, combining the two for the lookup. This is
the probe-amortisation trick ([[viewer-perf-probe-occlusion-skip]]) applied to
shadows, and it composes with the already-shipped off-thread, change-driven
caster cull ([[viewer-perf-shadow-cull-change-driven]]).

## Design

**Per cascade, keep two depth contributions:**

1. **Static cache** — all static casters' depth in the cascade's light-space
   projection. Rendered and retained; re-rendered only on invalidation (below).
   This is the expensive pass, run rarely.
2. **Dynamic pass** — only the dynamic casters (a handful), rendered every
   frame.

**Combine for the lookup** either by copying the cached static depth into the
frame's working map and rendering the dynamic casters on top (depth-test keeps
the nearer occluder), or by keeping two textures and taking the nearer occluder
at sample time. Per frame the added cost is a copy/blit + the few dynamic
casters, not a full shadow render.

### Why moving objects still receive static shadows (caster/receiver split)

A shadow map stores **caster depth in light space**, not "which surfaces are
lit". Shadows are *applied* in the main camera pass, every frame, per receiver
fragment (project the fragment into light space, compare depth). So caching only
freezes the static casters' depth; **receiving stays fully live**. An avatar
walking under a static building still darkens correctly — its fragments this
frame sample the cached static depth. All four combinations are correct as long
as the lookup samples the union of cached-static + fresh-dynamic occluders:

- static -> static: cached map.
- static -> dynamic (building shadow on an avatar): avatar shaded live, samples
  the cached static depth.
- dynamic -> static (avatar shadow on the ground): dynamic pass adds the avatar
  depth; the ground samples the union.
- dynamic -> dynamic: dynamic pass has all movers.

The failure mode this avoids (static shadows vanishing from movers) would only
happen if we cached the *final lit image* or skipped shading receivers; we cache
only static caster depth and shade every receiver live.

## Static vs dynamic classification (reaction, not prediction)

We cannot predict motion a priori (a script or a manual edit can move anything).
The clean split: classify a priori **only** the things that move **without a
server `ObjectUpdate`** (client-side animation); treat everything else as static
and invalidate it **reactively** off the `ObjectUpdate` that any real move
produces.

**A priori dynamic (must be in the per-frame dynamic pass):**

- avatars (pcode 47) and their attachments / rigged mesh (move with the
  skeleton),
- flexi prims (client-side sway),
- prims with non-zero `target_omega` (`llTargetOmega` client-side spin),
- physics-enabled (`FLAGS_USE_PHYSICS`) / keyframed prims if
  client-interpolated.

**Static, cached, reactively invalidated (the majority):** everything else,
**including scripted-but-still prims**. Do **not** use the "scripted"
`PrimFlags` bit as a dynamic marker — most scripts never move their object and a
huge fraction of any build is scripted; a scripted mover is caught by its
`ObjectUpdate` like any other move. Note
**texture-animated prims are shadow-static** (`llSetTextureAnim` moves UVs, not
the silhouette).

**Invalidation triggers (rare -> cheap):** an `ObjectUpdate` carrying a new
transform/shape for a cached caster (manual edit by another avatar, the rare
scripted move, rez/derez/reshape), or the sun angle drifting past a threshold
(WL day cycle — re-cache on a slow cadence / on delta). Manual edits "can touch
anything" but are rare and self-announcing — exactly what an event-driven cache
handles well.

**Live drags fall out for free** via a "moved-recently -> dynamic, settled ->
static" demotion: while a prim is being dragged it streams frequent
`ObjectUpdate`s, so it rides the cheap dynamic pass for the duration and the
static cache is untouched; when it settles (no updates for a window) it is
re-promoted and the static cache is re-baked **once**. Net: one static re-render
per *completed* edit, not per frame of the drag. This reuses the same lag
tolerance the async caster cull already relies on.

## Scope — the key design decision is camera-move resilience

Our cascades are re-fit to the camera each frame, so a cached static depth is
tied to one projection and a **camera pan invalidates it**. That sets a scope
ladder:

1. **Cached-CSM (core deliverable).** Cache static casters + per-frame dynamic
   pass + the classification/invalidation model above. Pays off whenever the
   camera is roughly still while the scene animates — the "parked in a busy sim
   / socialising" case, which is common in SL and currently pays full shadow
   cost.
2. **Texel-grid cascade snapping.** Snap each cascade's origin to its shadow-map
   texel grid so small camera moves don't shift the projection, extending the
   cache's validity across gentle panning. Cheap add-on to (1).
3. **Virtual shadow maps (maximal).** World-space shadow pages with per-page
   dirty tracking — fully camera-move-resilient (static pages persist across
   pans and only dynamic-touched pages re-render). A large graphics feature; the
   end-state if shadows dominate during active exploration, not a prerequisite
   for (1)/(2).

Deliver (1)+(2) first as a coherent unit; treat (3) as an explicit follow-up.

## Notes

- Composes with, does not replace, per-item/per-draw cost reduction (the wgpu
  texture-init fast-path, the shadow-cull sort removal,
  [[viewer-perf-per-object-face-merge-entity-count]]) — those cut the *dynamic*
  pass and the general render leg too.
- Interaction with [[viewer-perf-probe-capture-shadows]]: probe captures are
  currently shadow-free, so this is main-view only unless probes later take
  shadows.
- Builds directly on [[viewer-perf-shadow-cull-change-driven]] (the persistent,
  change-driven caster snapshot already distinguishes what changed — the natural
  place to hang the static/dynamic split and the cache-invalidation signal).

## Implementation (2026-08-17) — scope 1+2, always-on

Implemented as a single always-on path (no env modes — the deliverable is the
fully-on feature). `SL_VIEWER_SHADOW_CULL=off` still bypasses to stock Bevy for
A/B. Compiles; unit-tested for the classification split.

**Combine seam — dual-layer, single texture.** Each cascade is backed by **two
array layers** in the existing directional shadow texture: a per-frame *dynamic*
layer (even) and a retained *static* layer (odd). `shadows.wgsl`'s
`sample_directional_cascade` samples both and takes the nearer occluder (`min`
visibility) via a uniform branch on `GpuDirectionalLight.cascade_layer_stride`
(1 = stock single-sample; 2 = dual). No new bind-group binding — terrain and PBR
both go through the one shared function.

**Classification = "moved within a settle window".** Reuses the cull's existing
`Changed<GlobalTransform>` signal: a caster is dynamic while it has moved within
`DEFAULT_SETTLE_FRAMES` (30) of the current dispatch frame, else static. Client
animation changes its transform every frame, so it stays dynamic for free; a
settled object ages out and rejoins the retained bake. Transform re-propagation
that rewrites the *same* bounds does **not** reset the settle clock
(`caster_bounds_changed` epsilon compare), or a whole region would never settle.

**Scope 2 — the retained static map has its own persistent projection.** This is
what makes caching correct under camera motion (without it, a cached depth baked
under one cascade projection is sampled the next frame under a *different*
per-frame projection → misalignment → the shadows "cycle through objects" as the
camera pans — the flicker that scope 1 alone exhibited). In the fork,
`build_directional_light_cascades` also builds a persistent `StaticCascades`
(per view, per cascade): a **margin-expanded** (`STATIC_CASCADE_MARGIN` = 1.5),
texel-snapped projection that is **reused across frames** while the current
dynamic cascade's light-space coverage still fits inside it (and the sun hasn't
rotated), and only rebuilt — and its layer re-baked — when the camera leaves
that coverage. `GpuDirectionalCascade` carries a second `static_clip_from_world`
/ `static_texel_size`; the shader samples the static layer with **that** matrix,
so the retained depths stay aligned with the sample as the camera moves.

**Invalidation (two independent triggers).** A cascade's static layer re-bakes
when (a) its retained projection was just rebuilt (`StaticCascade::dirty`, the
fork's camera-left-margin / sun-rotated test) **or** (b) the viewer's static
caster *set* changed (order-independent XOR hash of the static entities → the
`CachedStaticShadows.bake_static` extract-resource). Projection invalidation and
set invalidation are deliberately split across the fork and the viewer — the
viewer no longer hashes frusta.

**The cull is dispatched early and applied late in the same frame.** This was
the decisive fix for the motion flicker. The off-thread caster cull used to run
dispatch-last / apply-first-next-frame, so its result reached the render
**one frame late**. The fork rebuilds the static *projection* with zero lag from
the current camera, so at a coverage rebuild the projection was a frame ahead of
the caster *set* the cull produced — the static bake mixed a new projection with
the previous frame's casters, and leading-edge objects blinked for a frame each
time the camera panned across a rebuild. The fix: `dispatch_shadow_cull` runs
**early** in `PostUpdate` (right after the cascade frusta are built) and
`apply_shadow_cull` runs **late** (before extract), so the pass — spawned off
the async pool — finishes in the gap and is applied in the **same** frame it was
dispatched (`apply` `block_on`s only in a rare overrun). The static caster set,
the static frusta it was culled against, and the static projection the render
samples are then all from the same frame. This keeps the off-thread parallelism
(no move to a synchronous cull) while removing the lag.

**Static casters are culled against the static frusta, not the dynamic frusta.**
This is the subtle bit that made the shadows flicker on camera pan even with the
persisted projection in place: the cull classifies casters into per-cascade
static / dynamic lists, and if the *static* list is frustum-culled against the
per-frame *dynamic* cascade frustum, a settled caster flickers in and out of the
retained bake as the camera pans (it keeps crossing the moving dynamic frustum
boundary), so its shadow blinks — and re-baking every frame does **not** help,
because the *set* is what churns. The fix: the viewer builds the static frusta
from the fork's retained `StaticCascades` (`Frustum::from_clip_from_world`) and
culls the static list against *those* (margin-expanded, stable), so a settled
caster stays in the bake while the camera moves within the margin. The static
set then changes only when the retained coverage itself is rebuilt — the same
coarse cadence as the projection re-bake.

**Render plumbing (`bevy_pbr`/`bevy_light`/`bevy_camera` fork).**
`prepare_lights` allocates `stride` layers per cascade; per cascade it fills the
static projection into `GpuDirectionalCascade` and pushes a *static* view-light
entity (subview index offset by `MAX_CASCADES_PER_LIGHT`, targeting the odd
layer) into `view_lights` **only** on bake frames — on clean frames the static
subview's pass does not run, so the odd layer is retained. The subviews baked
this frame are recorded in a render-world `StaticShadowBakes` resource that
`check_views_lights_need_specialization` reads to force a full re-queue of just
those subviews (a retained binned phase otherwise re-queues only the delta, so a
bake would render a partial set). `extract_lights` mirrors the dynamic
per-cascade population for the static casters from
`CascadesStaticVisibleEntities` (populated by our cull). Only the sun/main view
(render layer 0) has directional shadows — probe/pick/HUD/gizmo/water-exclusion
cameras are on non-zero render layers, so the sun builds no cascades for them
(`render_layers.intersects`) and there is no multi-view layer sharing.

### Status

Live-verified on OpenSim (2026-08-17/18): idle shadows correct and stable, and
the motion flicker is **gone** after the same-frame cull-ordering fix — static
objects keep their shadows as the camera pans. Fork unit tests cover
`calculate_static_cascade` reuse-vs-rebuild; the viewer's classification split
tests are updated for the static frusta.

### Perf measurement + tuning (Tracy, aditi 2026-08-18)

A Tracy capture on a dense aditi region (feature active, parked) showed the
first cut **regressed** frame time, from two costs the initial implementation
introduced:

1. **The caster cull ran on every camera, not just the sun view.** The fork
   builds sun cascades for *every* `Camera3d` (main + 6 probe-capture faces +
   gizmo / HUD / water-exclusion masks) but only renders shadows for views whose
   layers intersect the sun's. The viewer cull was frustum-testing all casters
   for all ~10 views, and — because the same-frame ordering fix makes `apply`
   `block_on` the cull — that ~11 ms landed on the main-thread critical path
   every frame. **Fix:** the cull now skips views whose render layers don't
   intersect the light's (matching `prepare_lights`). `apply_shadow_cull` p50
   **11.25 ms → 1.70 ms**; steady-state shadow subsystem ~15 ms → ~6 ms/frame.
2. **Every static bake force-requeues the whole set.** `specialize_shadows` /
   `queue_shadows` spike to ~45 ms on bake frames (the `StaticShadowBakes`
   force-wipe re-specializes all static casters), and a rezzing region bakes
   almost every frame as casters settle in. **Partial fix:** a bake debounce
   (`BAKE_DEBOUNCE_FRAMES`) coalesces the rez churn into an occasional bake
   (queue p90 47 → 33 ms). The per-bake cost is unchanged and remains a
   follow-up.

### Follow-up

- **Cut the per-bake cost:** re-specialize only the *changed* static casters on
  a bake instead of force-wiping the whole view (the
  `check_views_lights_need_specialization` path). The debounce reduces bake
  frequency but each bake is still ~45 ms.
- Re-measure with the viewer window **focused** — both aditi captures were
  present-throttled (occluded → ~2 fps), so per-system CPU is trustworthy but
  frame time is not.
- Scope 3 (virtual shadow maps) remains an explicit follow-up.
