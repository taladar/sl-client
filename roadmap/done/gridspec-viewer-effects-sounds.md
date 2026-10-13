---
id: gridspec-viewer-effects-sounds
title: Viewer effects and sound relays on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-viewer-effect-render, viewer-lookat-faithful,
  viewer-gesture-runtime, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-13)

Measured and written up in `book/src/gridspec/avatars.md` § Effects and
sounds.

- **Discover.** A new case, `effects-sounds`: one avatar sends, a second one
  beside it listens and then climbs away in three steps. Every effect type
  and two that are none; what is passed on of an effect (a batch, another
  agent's name, payloads that are not what the type says, odd durations, a
  repeat, a burst); a viewer's sound (gain out of range, no sound, positions
  far off and outside the region, another region's handle, a burst); each of
  a script's sound calls, by a probe script that does what its owner says on
  a channel; distance for all three; an arrival beside a looping prim; and a
  parcel that bars avatar sounds. Six runs on aditi and seven on OpenSim,
  with no answer that changed between two runs at the same spot; held to
  157 answers.
- **Findings.** Second Life sends an effect to **everybody, the sender
  too**, at any distance, under the sender's agent id whatever the viewer
  wrote; it drops floating text and a gaze, a pointing or a beam whose
  payload does not begin with the sender's id. OpenSim sends everything as
  it came to everybody but the sender, and a message of nothing but gazes
  and beams only to a viewer whose **camera** is within 10 m. A viewer's
  sound goes to the whole region on Second Life and to whoever is within
  100 m of the **stated** position on OpenSim, the sender included on both;
  the parent is nil on one and the sender on the other; gain, sound id,
  position and handle are passed on unread. **A parcel that bars avatar
  sounds lets none out, on either grid** — which is what the aditi sandbox
  does, and cost the first two aditi runs their sound legs. A script's loop
  is an `AttachedSound` and an update on Second Life, an update alone on
  OpenSim; a prim already looping is announced to an arrival by its update
  on both, with no `PreloadSound`.
- **Fake grid.** `relays.rs` and `ImitatedGrid::relay_policy`: effects and
  viewer sounds are published to the region and each session's watcher
  applies its flavour's rule (echo, the sender's name, what is dropped, the
  camera rule, the 100 m, the parent, the barred parcel). `SimState` keeps
  the camera an `AgentUpdate` last stated. Five timeline actions (`Effects`,
  `TriggerSound`, `AttachSound`, `AdjustAttachedSound`, `PreloadSounds`) and
  `SimSession::send_triggered_sound`, which states its own region handle.
  The case's viewer legs run offline on both flavours, held to 95 answers.
- **Viewer.** It already ignores an echo of its own gaze and pointing and
  copes with every order of sound messages either grid sends; a
  `SoundTrigger` with a nil sound now asks for nothing. `sl-client-tokio`
  re-exports the effect types it lacked.
- **Fixture.** `sound_location`, a spot of the build location's region that
  allows building *and* avatar sounds; the case measures the barred parcel
  at the build location and everything else there.

**Differences from the plan.**

- No e2e test of the viewer against the fake flavours: the viewer sends a
  pointing only from a crosshair key and plays no gestures, and it draws no
  beam or sphere, so there is nothing of its own to drive or to look at
  beyond what its unit tests cover. Rendering is
  [[viewer-viewer-effect-render]], the gaze [[viewer-lookat-faithful]], the
  gesture sound [[viewer-gesture-runtime]]; each got the measurements it
  needs.
- "`PreloadSound` on arrival" turned out to be nothing: neither grid sends
  one. What an arrival is sent was measured instead.
- OpenSim's barred parcel was measured in one run as the land's owner and is
  recorded, not held: the default run's avatar owns no land.

**Not measured:** a `SoundTrigger` down a child circuit; a parcel's *group
only* switch for a member ([[gridspec-aditi-test-land]]); a listener beyond
`llSetSoundRadius`; collision sounds; a sound from a child prim.

## Known already

OpenSim relays `ViewerEffect`; the fake grid relays neither effects nor client
`SoundTrigger`.

## Discover

Two avatars: B emits a beam / gesture sound, A records relay range, filtering,
owner and position; `PreloadSound` on arrival.

## Document

`book/src/gridspec/avatars.md` § Effects and sounds.

## Fake grid

Small — relays and timeline actions in this task.

## Viewer

Effects rendered, sounds from both paths.
