---
id: viewer-render-animation-coverage
title: Render-scene coverage — the animated avatar paths, which need their decode split from their transport
topic: viewer
status: ready
origin: split out of viewer-render-scene-coverage (2026-07), which covered every other path in its list; these three did not fit the registry's one rule
blocked_by: [viewer-render-scene-coverage]
refs: [viewer-render-scene-coverage, viewer-render-test-harness]
---

Context: [context/viewer.md](../context/viewer.md).

[[viewer-render-scene-coverage]] worked its list and landed thirteen scenes.
The last bullet of that list — **animesh / control avatars**, **body physics**,
**IK / locomotion** — is here instead, because writing the scenes for it found
that it is not the same kind of work. Every other path on that list was a spawn
function and a registry line. These three are not, and the reason is the
registry's own rule.

## Why they did not fit

The rule is that **a scene must not need a session**, and the task's own advice
on what to do when a path breaks it: "the fix is to separate the decode from the
transport, not to fake a session". Each of these three breaks it, in a way that
is a refactor per subsystem rather than a fixture:

- **Animesh.** `drive_control_avatars` reads `MessageReader<SlEvent>` directly:
  the set of animations an object is playing arrives as an event and is
  reconciled inside the driver. A scene can only say "this object is playing
  this animation" by writing an `SlEvent` into the app, which is faking a
  session — precisely what the rule forbids. The split is real and worth doing:
  `reconcile_playing` / `resolve_pose` / `retain_active` are already pure and
  already separate, so what is entangled is only the *ingest*.
- **Body physics.** `body_physics::apply` is nearly pure already, but what it
  produces is `AvatarRuntimeMorphs` entries and pose deltas — neither of which
  is geometry until the avatar pipeline in `avatars.rs` folds them back into a
  body, and that pipeline reads `AvatarState`, which a session fills.
- **IK / locomotion.** The same shape: `locomotion_ik`'s pure helpers are
  unit-tested, and what a scene would add is the *whole* posed avatar around
  them — the skeleton, the ground, and the agent's motion, which is where the
  near-singular leg of `sl-client-foot-ik-near-singular-leg` actually lives.

All three additionally need `SL_VIEWER_ASSETS` (there is no skeleton without the
Linden `character/` directory), which `avatar-morphed-body` shows is a payable
cost — but paying it on top of an event-transport fake would be two compromises
for one scene.

## What a scene here would be worth

The most, of anything left. These are the three paths in the viewer that are
**time-varying and verified only by a login**, and the timeline tier is built
exactly for them: `avatar-morphed-body` covers the shape at rest, and nothing
covers the shape in motion — which is where R11 hid for a whole session behind
an R13 that looked like a different bug.

## Shape of the work

One subsystem at a time, and the animesh one first because its split is the
cleanest:

1. Give `crate::animesh` an ingest system that reads the events and a driver
   that takes the reconciled set, the way `crate::objects` already separates
   `apply_object` from the event loop. Then a scene declares the playing set
   directly.
2. A scene per subsystem, each dynamic, each declaring a timeline it must move
   over. The `.anim` assets the animesh scene needs are decodable from bytes
   (`sl-anim` is sans-I/O) — but see the note in `context/viewer.md`: stock
   Firestorm ships **no** `.anim` files, so a procedural fixture or the OpenSim
   `AnimationsAssetSet` is the source, not `SL_VIEWER_ASSETS`.

## What to watch for

- **HUD went the same way and is not in this file.** `setup_hud_screen` needs
  the library's HUD attachment-point table *and* a `Window` (`fit_hud_points`
  reads the aspect ratio from it), and its camera is spawned at the world root
  rather than under a scene root — so a HUD scene in the gallery would fight the
  gallery's own camera. It is a smaller job than these three and belongs with
  whoever next touches `hud.rs`; the pure half (`anchored_point_offset`) is
  already unit-tested there.
