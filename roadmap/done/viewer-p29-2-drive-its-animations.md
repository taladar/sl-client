---
id: viewer-p29-2
title: Drive its animations
topic: viewer
status: done
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
~~**Next step:** a `tcpdump` of an aditi session run through
`sl-conformance-trace` to correlate the `ObjectAnimation.object_id`s against
`ObjectUpdate` ids.~~ Superseded by the code investigation below: it is "we
track them but key them wrong", and the wire capture is no longer needed to
settle the keying question.

## Investigation (2026-07-22): root cause found

A source-level pass over our code, the Firestorm reference, and the OpenSim
server found two viewer-side defects that together explain all three live
observations. Findings only — no fix applied yet.

### Defect 1: `ObjectAnimation.Sender.ID` is a *part* UUID, not the root

The sim keys `ObjectAnimation` by the **linkset part that holds the
animations** (the prim the script runs in), which is often a *child* — not
the animesh root:

- OpenSim writes `sop.UUID` of the part carrying `Animations` into the
  Sender block (`LLClientView.cs:5589-5590`, inline send path in
  `ProcessEntityUpdates`); the send is gated on the **root** carrying the
  ExtendedMesh `0x70` param (`root.Shape.MeshFlagEntry`,
  `LLClientView.cs:5576`) and on the viewer having requested the
  `ObjectAnimation` seed cap (`BunchOfCaps.cs:430`).
- Firestorm's `process_object_animation` (`llviewermessage.cpp:5345`)
  stores the signalled set **per part UUID** in the persistent
  `LLObjectSignaledAnimationMap` singleton (`llviewermessage.cpp:5366`),
  and `LLControlAvatar::updateAnimations` (`llcontrolavatar.cpp:562,579`)
  **merges the maps of every volume in the linkset** into the control
  avatar — a child-keyed animation legitimately drives the root's control
  avatar.
- Our code requires the sender id to *be* the flagged animated root:
  `drive_control_avatars` keys `control.playing` by the raw sender id
  (`animesh.rs:245`) and only poses when `control.avatars` — keyed by the
  animesh **root** — contains that same id (`animesh.rs:262`);
  `spawn_animesh_control_avatars` filters `tracked.animated &&
  control.is_playing(tracked.full_key)` (`objects.rs:2991`), which a
  child-keyed event can never satisfy (the child is not `animated`, the
  root is not `is_playing`). The `ControlAvatarState` doc comment
  (`animesh.rs:73-74`) asserts the wrong invariant outright. The
  child→root walk already exists (`animesh_root`, `objects.rs:2705`) but
  is only used for mesh binding, never for animation folding.

This makes the second and third live observations **the same events**: the
Mario's animations are keyed to the child part holding them (logged as
"linkset children with `animated=false`"), which is exactly why the tracked
roots show zero `ObjectAnimation`.

### Defect 2: `prune_control_avatars` destroys the early-arrival buffer

`prune_control_avatars` (`objects.rs:3004-3013`) runs every frame and
`ControlAvatarState::retain` (`animesh.rs:190-194`) drops `avatars`,
`playing`, and `poses` for any key not currently tracked. An
`ObjectAnimation` arriving before the object's first `ObjectUpdate` is
folded into `playing` and pruned the same frame — the event cursor has
advanced, so it is gone for good. That defeats the documented early-spawn
intent ("an animation that arrives before the mesh decode is not lost",
`animesh.rs:136-140`) for the not-yet-tracked case, i.e. all ~15/17 of the
first observation. Firestorm keeps its signalled map indefinitely and
re-reads it whenever a control avatar is (re)built.

The ~15/17 never-streamed objects themselves are most likely attachments of
avatars hidden by the **parcel privacy option** on the parcel our avatar
stood on (the "see/interact with avatars on other parcels" setting unset):
hidden avatars are never streamed as objects, so neither are their
attachments — yet the sim still sends their `ObjectAnimation`s. Sim-side
behaviour, not our bug; a future live test should pick a parcel without
that restriction.

### Confirmed correct (do not re-tread)

- `ObjectAnimation` is in the seed-caps request
  (`sl-proto/src/session.rs:505`); OpenSim gates on exactly that string.
- ExtendedMesh `0x70` parses on the full **and** compressed update paths
  (`sl-proto/src/extra_params.rs:58,106,477`; compressed tail
  `compressed.rs:299-302`); `animated` is set independent of `is_root`
  (`objects.rs:325-337, 2108, 2190`).
- `ObjectUpdateCached` → `RequestMultipleObjects` cache-miss refetch works
  (`methods.rs:1683-1706`); `RegionHandshakeReply` flags=0 is a valid
  cacheless choice; `AgentUpdate` carries a live camera and Far=512 m.
- Dispatch passes `Sender.ID` through faithfully on root and child
  circuits (`methods.rs:1593-1605, 3197-3208`); full-UUID keying is
  consistent end to end (no local-id confusion).

### Fix direction

Mirror Firestorm: keep the signalled-animation sets **per part UUID** in a
persistent map that survives the object being untracked (drop only on
region handoff / session end, or bound it); at drive/spawn time resolve
each signalled part through `animesh_root` and **merge all parts of a
linkset** into the root's control avatar; spawn the control avatar early
when *any* part of an animated linkset has an animation. Stop pruning
`playing` by tracked-object liveness.

### Fix implemented (2026-07-23) — verified live

Both defects fixed along the triaged fix direction, mirroring Firestorm:

- **`ControlAvatarState.playing` now keys by the signalled *part*** (the
  `ObjectAnimation` sender), documented as such, and is **persistent**: it
  survives the part being untracked (`prune_control_avatars` no longer
  touches it; the reference's session-lifetime
  `LLObjectSignaledAnimationMap`), with a `MAX_SIGNALLED_PARTS` (4096)
  safety cap that, only when exceeded, drops never-tracked parts. The
  early-arrival buffer therefore survives arbitrary arrival order.
- **Part → root resolution + linkset merge**: `drive_control_avatars`
  resolves every signalled part up its linkset via `animesh_root` (bulk
  full-key→scoped lookup `ObjectState::scoped_by_full_keys`, one pass) and
  merges all parts' sets into the root's control avatar before blending —
  the reference's whole-linkset `updateAnimations` merge. A child-keyed
  animation now drives its flagged root.
- **Early spawn keys on the linkset, not the root**:
  `spawn_animesh_control_avatars` spawns a control avatar for every root
  any of whose parts is signalled, resolved the same way.

**Verified live on aditi (2026-07-23): the Mario animeshes animate.** One
follow-up filed from the same session: each Mario sits inside an
almost-transparent box shell ([[viewer-animesh-transparent-box-shell]]),
likely the root prim's own transparent geometry vs. the reference's cull.

### Local OpenSim repro (removes the aditi dependency)

Animesh works out of the box on a default standalone: `SimulatorFeatures`
advertises `AnimatedObjects` unconditionally
(`SimulatorFeaturesModule.cs:154`) and `llStartObjectAnimation` /
`llStopObjectAnimation` / `llGetObjectAnimationNames` are plain `LSL_Api`
(`LSL_Api.cs:4219`). A test object needs: a root prim whose OAR
`<ExtraParams>` base64 contains a `0x70` entry with flags=1 (OpenSim treats
mere *presence* of the block as animesh-capable,
`PrimitiveBaseShape.cs:1378-1384`, but our client checks bit `0x1`), a
rigged mesh, and either a script calling `llStartObjectAnimation` (the
animation asset must be in that prim's inventory; XEngine enabled per the
scripted-object testing memory) or a hand-crafted `<SOPAnims>` element
(base64 blob: uint16 count, then per entry a 16-byte UUID, 1-byte name
length, name bytes). Putting the script in a **child** prim reproduces
Defect 1 deterministically.
