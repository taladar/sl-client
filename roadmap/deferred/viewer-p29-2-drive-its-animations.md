---
id: viewer-p29-2
title: Drive its animations
topic: viewer
status: deferred
origin: VIEWER_ROADMAP.md — Phase 29 — Animesh
---

Context: [context/viewer.md](../context/viewer.md).

**P29.2. Drive its animations.** Route the object's animation state
(`ObjectAnimation`) through the Phase 18 blend driver against that skeleton so
the rigged mesh deforms. Reuses the Phase 12 skeleton and Phase 18 blend.
Reference: `LLControlAvatar` / `LLDrawPoolAvatar`. **Implemented but NOT yet
observed animating live — blocked on `ObjectAnimation` delivery / object
tracking, needs a wire-capture investigation.** The driving pipeline is in
place and correct: the three per-avatar animation helpers were extracted from
`animations.rs` as shared `pub(crate)` functions — `reconcile_playing` (now
taking `(anim_id, sequence_id)` pairs so both `PlayingAnimation` and
`ObjectPlayingAnimation` drive it), `retain_active`, and `resolve_pose`
(sample + priority-blend a playing set into an `AnimationPose` with a
joint-name→index resolver) — and the avatar driver now calls them too, so the
animesh path shares the exact ease-in/out + priority-blend logic.
`ingest_object_animations` fetches each signalled motion through the **same**
`AnimationManager`; `drive_control_avatars` folds each object's
`ObjectAnimation` into a per-object playback clock and blends a pose (names
via the shared `AvatarBody::joint_index`); `pose_control_avatars` (in
`PostUpdate`, after propagation, beside `pose_avatar_skeletons`) re-runs the
SL skeletal recurrence with a **rest** `SkeletalDeformations` + the linkset's
joint overrides and writes each joint's world matrix.
`spawn_animesh_control_avatars` spawns a control avatar as soon as an object
has an animation playing (not only when its mesh binds), so an animation
arriving before the mesh decode is not lost. **Live-verified on
fetch/decode:** the signalled custom `.anim` motions fetch and decode fine (no
errors). **But no animesh actually animates**, because the `ObjectAnimation`s
the sim sends do not correspond to the animesh we track and render:

- of the animated objects an aditi region signalled, **~15 of 17 were never
  tracked** by us at all (an `ObjectAnimation` arrives but no `ObjectUpdate`
  ever does) — most likely animesh **attachments on the coarse / distant
  avatars** (whose wearer is not streamed as a full object, so neither are its
  attachments), since the region had no fully-rendered neighbour avatars;
- the few we *do* track are **linkset children with no animated flag**
  (`is_root=false, animated=false`), so `animesh_root` / the early-spawn never
  key a control avatar to them; and
- the in-world Mario animesh we *do* track as animated roots (and spawn
  control avatars for) receive **zero** `ObjectAnimation`, even after the
  capability fix below — so the sim is not streaming their (looping, set-once)
  animation to us.

Fixes made along the way that **did** land (all build/clippy/test clean, no
OpenSim login regression): (1) the viewer now requests the **`ObjectAnimation`
capability** in its seed-caps list (`CAP_OBJECT_ANIMATION`) — the sim
withholds the `ObjectAnimation` UDP stream from a viewer that did not
advertise animesh support, which is why we saw *zero* animation events before;
this made many more arrive. (2) `Session::dispatch_child` now handles
**`AvatarAnimation` / `ObjectAnimation` on child (neighbour-region) circuits**
— they were falling through to the unhandled-message diagnostic, so
neighbour-region avatars and
animesh could never animate. (3) `CompleteAgentMovement` is now **deferred
until the region's capabilities are fetched** (both runtimes) so the sim knows
we render animesh before it streams the scene — did not by itself unblock the
Mario, but is correct in general and fails login cleanly if caps never arrive.
**Next step:** a `tcpdump` of an aditi session run through
`sl-conformance-trace` to correlate the `ObjectAnimation.object_id`s against
`ObjectUpdate` ids — to settle "the sim never streams these objects to us" vs.
"we track them but key them wrong", and to see why the tracked Mario roots get
no `ObjectAnimation`.
