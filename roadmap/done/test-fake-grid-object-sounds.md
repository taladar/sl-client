---
id: test-fake-grid-object-sounds
title: The fake grid can serve a sound but cannot play one
topic: test
status: done
origin: noticed while doing test-assets-sound-encoder (2026-09-01)
points: 3
refs: [test-assets-sound-encoder, test-fake-grid-builtin-sounds]
---

Done 2026-09-08.

Context: [context/testing.md](../context/testing.md).

[[test-assets-sound-encoder]] gave the workspace a sound asset a fixture can
write and a fake region can serve by id. What it did not give it is a sound
anyone *hears*: nothing on the grid side ever told a viewer to play one, so
`sl-viewer-audio`'s `world_sounds` — the in-world half of a viewer's audio,
already shipped — had no fixture driving it at all.

## The second question, answered: it is not two paths, it is four messages

The task assumed the simulator's whole vocabulary was messages. It is not, and
the assumption was hiding the bug. A simulator states an in-world sound in two
unrelated places, and **which one it uses depends on whether the sound loops**:

- `llLoopSound` writes `Sound` / `Gain` / `Flags` / `Radius` **onto the prim**
  and schedules a full object update. OpenSim's `SoundModule::LoopSound` says
  why in a comment above it: *"just sending the sound out once doesn't work so
  well when other avatars come in view later on"*. `llStopSound` goes the same
  way — `Sound` nil, `SoundFlags::STOP` — never as a message.
- `llPlaySound` (non-looping) is the `AttachedSound` **message**;
  `llSetSoundVolume` on a playing sound is `AttachedSoundGainChange`;
  `llTriggerSound` and a collision are `SoundTrigger`; `llPreloadSound` is
  `PreloadSound`. Each reaches only whoever is already standing there.

So the answer to "should the viewer also honour the object-update fields" is
that it is not an *also*: it is the **only** way an avatar arriving after a loop
started ever learns of it, which is nearly every avatar in every region. The
reference reads them in `LLViewerObject::processUpdateMessage`, in the full and
the compressed update alike, and funnels both sources into one
`setAttachedSound`. `world_sounds` now does the same through
`apply_attached_sound`, which is also where the reference's subtler rules live:

- restating the same looping sound (which a moving prim does on **every**
  update) adopts the new gain in place instead of restarting the loop;
- a nil sound clears a *looping* source but leaves a one-shot to finish, so an
  ordinary update for a soundless prim does not cut off the `llPlaySound` it
  was just told to play;
- the update's `Radius` gates by distance from the ears
  (`LLAudioSource::checkCutOffRadius`), driven to silence rather than stopped,
  the way the parcel-local clamp already was. Zero is "no cutoff".

One deliberate departure, written down beside the code: a `STOP` arriving *with*
a sound id stops it here, where the reference's null check returns before it
looks at the flag and would start the sound. Nothing sends that.

Finding this also fixed a latent bug the message path had: an attached sound
whose object was not (yet) in `ObjectState` was stopped and forgotten on the
next frame. A looping sound arrives **on** the object's own update, so "no
entity yet" is the normal first frames of every one of them. The voice is now
silenced but the record kept, for `ATTACHED_ORPHAN_SECONDS`.

## What landed

- `SimSession::{send_sound_trigger, send_attached_sound,
  send_attached_sound_gain_change, send_preload_sound}` — the server-sent half
  of all four, reliable as OpenSim sends them, each doc'd with the script call
  it comes out as. Loopback-exercised in `tests/sim_session.rs`
  (`simulator_sounds_reach_client`), including the nil-plus-`STOP` stop.
- `PrimFixture::looping_sound(sound, gain, radius)`: the fixture shape for
  "this prim has been looping this clip since before you arrived".
- The catalogue's nineteenth prim, `sound-box`, looping
  `sound::marker_tone(tones::MID)` at `SOUND_CLIP` — served like every other
  catalogue asset, and given a landmark for free (they derive from `entries()`).
- `client_end_to_end::the_catalogue_sound_box_loops_a_fetchable_clip`: the
  arrival burst carries the sound, its gain, its `LOOP` flag, its radius and a
  non-nil owner (one of the two ids a mute names); the clip fetches back byte
  for byte; and an `AttachedSound` driven from the grid side arrives as
  `Event::AttachedSound` not marked as a loop.

**No `PreloadSound` on arrival**, because no real region sends one: OpenSim's
`SoundModule::PreloadSound` is reached from `llPreloadSound` and nothing else.
That was the one open question in the task's own acceptance, and the answer is
"the region says nothing".

[[test-fake-grid-builtin-sounds]] is still a different gap — the built-in
*library* ids the viewer asks for on arrival — and neither blocks the other.
