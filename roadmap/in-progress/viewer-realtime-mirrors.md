---
id: viewer-realtime-mirrors
title: Real-time mirrors (hero probes)
topic: viewer
status: in-progress
origin: render-feature gap analysis vs Firestorm (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

Actual mirrors — a surface that reflects the scene (and *you*) in real time,
re-rendered every frame. SL added this as the **"hero probe"**: a reflection
probe that, unlike the static/blurry P33 probes, is rendered fresh per frame
from the mirror plane, so it is sharp and live. It is what makes a bathroom
mirror or a shop mirror-wall work.

Firestorm gates it on `RenderMirrors`, with `RenderHeroProbeResolution`
(sharpness) and `RenderHeroProbeUpdateRate` (how often it re-renders — the perf
lever).

Scope: identify mirror surfaces (the material/flag that marks a face as a hero
reflector), render the scene from the reflected camera into the probe target
each frame (or every N frames per the update rate), and sample it on the mirror
face. This is expensive — a second scene render per active mirror — so the
instance cap and update-rate throttle are part of the feature, not an
afterthought.

Reference (Firestorm, read-only): the hero-probe path, `RenderMirrors`,
`RenderHeroProbe*`.

Builds on: the P33 reflection-probe infrastructure (this is its dynamic,
per-frame cousin).

## In progress (2026-08-08)

Implementation landed; **kept in-progress pending a live visual on a real
reflective mirror *surface*** (see "Verification" below). Implemented in
`sl-client-bevy-viewer/src/probes.rs` (a "hero probes" section at the bottom of
the module), as the dynamic cousin of the P33 reflection probes it reuses
wholesale.

**The unblock.** This was filed `blocked_by: viewer-perf-probe-scheduling`, but
that dependency turned out not to be real for the hero path and the blocker was
dropped. The zero-idle *change-driven* scheduling rework is about the amortized
one-face-per-frame *local/ambient* pool; a hero probe deliberately does **not**
use that pool's cadence — it runs its own every-frame, all-six-faces capture,
independent of the P33 `CaptureSchedule`. The P33 pieces it actually needs (the
`CaptureRig` cube + six face targets, the render-world face→cube blit, the
`LightProbe` holder + `GeneratedEnvironmentMapLight` volume, the exposure
calibration, the shadow-free probe render layers + mirror sun) already exist and
are stable. So the hero probe layered straight on top of them.

**What a mirror is.** A reflection-probe prim carrying the `MIRROR` flag (P33
already decodes it into `ReflectionProbe.flags`; it lifts to
`ObjectReflectionProbe::is_mirror()`). No new wire/proto work.

**How it captures.** A small pool of hero rigs (`MAX_HERO_PROBES`, the instance
cap — 1 for now) is handed to the nearest mirror prim(s), same ranking as the
P33 local pool. Each rig is a `CaptureRig` at the configurable
`RenderHeroProbeResolution` (default 512² per face, power-of-two-clamped to
[128, 2048] — well under the reference's 2048 default to keep VRAM/fill
tractable). Every `RenderHeroProbeUpdateRate` frames (default 1 = every frame)
all six hero cameras are posed at the mirror origin and activated, so the whole
cube re-renders that frame — sharp and live, dynamic content (avatars) always
included. Between updates (rate > 1) and when no mirror is in view the cameras
sit inactive, so an idle scene pays nothing.

**How it lands on the glass.** The hero rig's `LightProbe` volume sits at the
mirror prim (floored to `HERO_MIN_VOLUME_EXTENT` per axis so a *flat* mirror
still has depth in front of it), so Bevy's per-fragment probe lookup finds the
sharp hero cube for the mirror surface, overriding the default probe there. To
stop the two families fighting over the same surface, mirror prims are excluded
from the P33 local pool while `RenderMirrors` is on (`rank_local_probes`'s new
`exclude_mirrors`); toggling `RenderMirrors` off releases the hero rigs and lets
the P33 pool reclaim the prims as ordinary (blurry) probes.

**The reference bug we avoid.** Firestorm's hero pass renders non-rigged (rigid)
attachments at a stale pose, so they float free of the avatar in the glass — a
mirror of the bug we once had on the avatars themselves (the pose driver
orphaning joint children). We avoid it *structurally*: the hero cameras render
the same live ECS entities as the main view at the same `GlobalTransform`s (no
separate mirror-pose pass), so a rigid attachment placed by
`pose_attachment_nodes` before the render is exactly where the main view has it,
and tracks the avatar in the reflection. The invariant, documented in the
module: a hero capture must never pre-pose the scene.

**Settings** (persistent, `[render]`, live-synced except resolution which sizes
GPU targets → restart): `render_mirrors`, `render_hero_probe_resolution`,
`render_hero_probe_update_rate`. File-based like the sibling P33
`render_reflection_probe_dynamic_content`; there is no Graphics preferences tab
yet to surface them (a natural follow-up).

## Verification

**Unit** (`cargo test -p sl-client-bevy-viewer probes::`, 13 pass): the
`MIRROR`-flag lift, the power-of-two resolution clamp, the flat-mirror volume
floor.

**Headless GPU render — the decisive end-to-end check** (passed, 5.0 s, a real
render not a skip): `render_readback`'s
`the_mirror_reflects_each_neighbour_on_its_own_side` renders the
`metallic-sphere-among-prims` scene and asserts the red/green/yellow neighbours
each reflect on the correct side of the sphere. That scene's probe is
`MIRROR`-flagged, so with mirrors on it is **routed to the hero path** (excluded
from the P33 pool), and the default probe is environment-only — so the coloured
prims can only appear in the sphere via the hero capture. The test passing means
the hero rig captures the scene and the surface samples the sharp cube with the
correct orientation. This is the client-side proof the render chain is right.

**Live (aditi)** — the detect → bind → capture → release lifecycle ran clean
over ~14 min with no panic: the landing region had a `MIRROR`-flagged probe, the
hero path bound it (`mirror took hero rig 0`, `1 mirror(s) captured live`) and
released it on navigating away (`released hero rig 0`).

**Still pending** — a live *visual* on an actual reflective mirror **surface**
(a low-roughness face inside a mirror probe). Aditi content largely predates
mirrors so none was found to eyeball; the headless render check above stands in
for the render correctness, but a screenshot of a real mirror reflecting the
avatar is the remaining confirmation before this moves to `done`. A provisioned
OpenSim mirror (rez a prim as a mirror-flagged probe with a reflective PBR face)
is the fallback if no aditi mirror content turns up.
