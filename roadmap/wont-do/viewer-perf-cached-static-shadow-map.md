---
id: viewer-perf-cached-static-shadow-map
title: Cache the static sun-shadow map; re-render only dynamic casters
topic: viewer
status: wont-do
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

## ⛔ Wont-do — reverted (2026-08-18): incompatible with SL texture LOD

Built and reverted. The cached-static split (retained static cascade + per-frame
dynamic pass, forked into `bevy_pbr`) is **fundamentally at odds with Second
Life's progressive texture LOD**, and no fix landed after six attempts. Reverted
to stock Bevy shadows behind the (retained, working) off-thread caster cull.

**Root cause.** SL streams textures coarse-first and then
*refines the discard level as the pixel area changes* (`textures.rs`, P21.1).
Each refine builds a new `Image` and
**swaps the face material's `base_color_texture` handle**, which fires
`AssetChanged<MeshMaterial3d>` — so the fork's
`check_entities_needing_specialization` re-specializes the prim. That
despecializes it from the shadow pipeline cache **and** dequeues it from the
retained static bins, and while the new LOD texture is still decoding the
material's `PreparedMaterial` is transiently absent, so `specialize_shadows`
bails and the caster spec-misses. The retained static phase has
**no tolerance for a one-frame specialization gap and no dynamic fallback** (the
caster is, by design, not in the per-frame pass), so its shadow vanishes for a
whole bake period and re-appears when it settles — while others drop. Because
LOD refines as the camera moves, it is camera-motion-correlated, per-prim, and
rotating; on prims that never move. The pure-dynamic path is immune because it
re-renders every caster every frame, so a one-frame spec gap is invisible.

Confirmed live on aditi: a pure-dynamic A/B (`SL_VIEWER_SHADOW_ALL_DYNAMIC`)
removed the disappearing + seams (the black-shadow symptom is separate — a
lighting/ambient issue present without the feature, filed elsewhere). A headless
reproduction (a static caster platform under a panning camera) stayed clean —
the bug needs real FaceMaterial LOD churn, which the minimal scene lacks.

**Six fix attempts (all failed; preserved in the fork WIP commit `e1583d0f2`):**
(1) a `mix64` avalanche on the static-set change hash — a real bug (an aligned
run of freshly-spawned entity ids XOR-cancelled to zero so a populated set
hashed like the empty set and never re-baked), but not the cause; (2) removed
the per-bake force-requeue (the retained bins are in fact complete for loadable
casters); (3) synchronized all four cascade re-bakes; (4) route a queue-time
spec-miss to `pending`; (5) retained-phase tolerance (`add` upsert +
`iter_to_dequeue_retained` so a still-visible caster is not dropped on a pure
material change); (6) an `Arc<MaterialProperties>` cache in `specialize_shadows`
to survive the transient `PreparedMaterial` gap. Each addressed a plausible
layer and the visual symptom survived every one; the log-vs-visual correlation
was never conclusively confirmed.

**If revisited:** first *prove* which mechanism produces the visual drop (tag
one known-disappearing prim and correlate its shadow to the trace) before
writing any fix; and it likely cannot work as a retained phase without either
decoupling shadow specialization from texture-content material changes, or
giving a recently-despecialized static caster a dynamic-pass fallback until its
static representation is guaranteed. Scope 3 (virtual shadow maps) would
sidestep the per-cascade-projection coupling but is a large graphics feature.
The off-thread caster cull ([[viewer-perf-shadow-cull-change-driven]]) is
unaffected and stays.

The original design and analysis below are retained for a future attempt.

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
