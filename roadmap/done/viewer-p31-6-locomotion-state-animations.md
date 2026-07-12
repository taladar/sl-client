---
id: viewer-p31-6
title: Locomotion / state animations
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Simulator authority & the Firestorm motion model (read before P31.2)
---

Context: [context/viewer.md](../context/viewer.md).

**P31.6. Locomotion / state animations.** Drive the avatar's
built-in state animations — the ones an animation overrider (AO) replaces
— from its movement state: **standing**, **walking** / **running**,
**turning left / right**, **flying** / **hovering**, **falling**,
**jumping**, **crouching**, **sitting**. On the wire the simulator drives
these: it plays a state animation on the avatar (e.g. `ANIM_AGENT_WALK`)
and pushes the set via `AvatarAnimation`, which the viewer already ingests
and plays through the Phase 18 pipeline. That pipeline is already proven —
on **aditi** the viewer has played the sim's standing animations in past
runs — so the first task is to find why the **local OpenSim** P31.4/P31.5
run showed no locomotion animation on the moving avatar: whether OpenSim
drives `AvatarAnimation` for these states at all, and whether the received
set is reaching the play path for the own avatar. Only then add a
**client-side** fallback (derive the state from the P31.4 velocity / the
movement `ControlFlags` and play the corresponding built-in `.anim` from
the `character/` set) for the own avatar's immediate feedback / where the
sim does not drive it. Reference: Firestorm `LLAgent` motion controllers
and `llvoavatar` `ANIM_AGENT_*` ids. **Done — the investigation found the
planned premise wrong in two ways, so the fix is two library bug-fixes plus
a viewer fallback, not the assumed synthesis / local `.anim`.** (1) **Root
cause: an `sl-anim` registry misclassification.** walk / run / stand /
turn / crouch (and the female / `*_new` / `standup` variants) were marked
`BuiltinKind::Procedural` (no downloadable asset), so the resolver skipped
the fetch and *never played them*. But the reference viewer's
`LLKeyframeWalkMotion` / `LLKeyframeStandMotion` / `LLKeyframeFallMotion`
all **extend `LLKeyframeMotion`**, which downloads the keyframe asset by
UUID (`gAssetStorage->getAssetData`) and only layers a procedural
*adjustment* (foot IK / torso facing) on top — the assets are ordinary
downloadable `.anim`s (confirmed: OpenSim serves them under the exact
built-in UUIDs, e.g. `walk` `6ed24bd8…`). Fixed by reclassifying the 17
locomotion entries `Procedural → Keyframe`; the genuinely procedural ones
(the `LLEmote` `express_*`, `do_not_disturb`, and the always-on adjusters —
which the sim never signals and are absent from the table) stay
`Procedural`. Added `sl_anim::builtin_animation_by_name` (name → built-in)
for the viewer's state → UUID lookup. So there is **no local `.anim` from
`character/`** and **no synthesis** — the built-ins download from the grid
like any other, and the sim-driven path (the primary one, per the note
above) then works. (2) **A second, latent P18.4 bug the fix exposed:**
`reconcile_playing` stamped a dropped animation's `stopped_at` with the
**absolute wall clock** (`now`), while `Motion::pose_weight` /
`is_finished` compare it against `elapsed = now - start` (**motion-elapsed**
since that animation started). A *non-looping* motion was saved by its
natural ease-out (`min(stopped_at, duration - ease_out)` picks the smaller),
so gestures always faded correctly and the bug stayed hidden — but a
*looping* locomotion motion has no natural ease-out, so a stopped walk held
full weight until `elapsed` reached the (large, ever-growing) wall-clock
value: it "stuck" on walk for a few seconds early in a session and
effectively forever later. Fixed by storing `stopped_at = now - start`
(the documented motion-elapsed timeline); regression-tested. (3)
**Client-side fallback (`locomotion.rs`, viewer-only):** derives the own
avatar's state from the P31.5 advertised `ControlFlags` **intent** (walk /
run / turn / fly / ascend / descend) plus the P31.4 dead-reckoned
*vertical* velocity (fall / fly-vertical only), maps it to the built-in
animation via `builtin_animation_by_name`, and plays it on a new
client-driven slot on `AnimationPlayback` — **deferring entirely** while the
sim is driving the avatar (a new `has_active_sim_animation` gate), so the
two never double-drive. Intent (not velocity) drives walk/run/turn on
purpose: the intent clears the instant the key is released, whereas the
last-reported velocity lingers at walk speed until a corrective update — the
same "doesn't stop when you stop" trap in miniature. Diagnostics:
`SL_VIEWER_LOG_LOCOMOTION=1` (edge-logged state + the sim's per-update
`AvatarAnimation` set) and `SL_VIEWER_FORCE_CLIENT_LOCOMOTION=1` (force the
fallback on to exercise it on a root presence). Kept viewer-only (no
runtime-parity obligation); `sl-anim` is the only shared-library change.
Verified **live on OpenSim** (user-confirmed on screen): walk / stand fetch,
decode, and pose the skeleton; the own avatar walks and settles back to
standing **promptly** on key release (the wire log shows OpenSim broadcasts
clean `walk#n ↔ stand#n+1` transitions and the reconcile now eases the walk
out within its ~0.5 s ease-out); this login was a root presence so the sim
drove it and the client fallback correctly stayed deferred. **Scope: only the
base keyframe motion plays.** The reference viewer's *procedural adjustment*
overlay — the whole point of the `LLKeyframe*Motion` subclasses — is **not**
ported: `LLKeyframeWalkMotion`'s playback-speed match to ground velocity +
`LLWalkAdjustMotion` foot-plant IK / pelvis lag (anti-foot-skate),
`LLKeyframeStandMotion`'s lower-body twist to face the look direction with
foot IK, `LLKeyframeFallMotion`'s landing recovery, and the always-on
adjusters the sim never signals (`LLHeadRotMotion` head-track, `LLEyeMotion`,
`LLHandMotion`, `LLBodyNoiseMotion` idle sway, `LLBreatheMotionRot`). So feet
can skate, the stand does not turn its legs to the look direction, and there
is no head/eye/breathe idle motion → **P31.8**. **Follow-up noted:** avatar
turning is not interpolated like translation, so it reads choppy → **P31.7**
(a motion-smoothing gap, unrelated to the animations).
