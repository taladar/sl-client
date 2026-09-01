---
id: test-fake-grid-object-sounds
title: The fake grid can serve a sound but cannot play one
topic: test
status: ready
origin: noticed while doing test-assets-sound-encoder (2026-09-01)
points: 3
refs: [test-assets-sound-encoder, test-fake-grid-builtin-sounds]
---

Context: [context/testing.md](../context/testing.md).

[[test-assets-sound-encoder]] gave the workspace a sound asset a fixture can
write and a fake region can serve by id. What it did not give it is a sound
anyone *hears*: nothing on the grid side ever tells a viewer to play one, so
`sl-viewer-audio`'s `world_sounds` — the in-world half of a viewer's audio,
already shipped — has no fixture driving it at all.

The two paths a simulator uses are **messages**, not object fields:

- `SoundTrigger` — a one-shot at a fixed position (`llTriggerSound`, a
  collision, a neighbouring region's sound). `SimSession` handles the
  *client-sent* form (a viewer triggering one) at `sim_session.rs:9667` but
  has no sender for the server-sent form.
- `AttachedSound` — a sound bound to an object (`llPlaySound` /
  `llLoopSound`), followed by `AttachedSoundGainChange` for a live volume
  change and a fresh `AttachedSound` carrying `SoundFlags::STOP` to end it.
  Nothing sends any of the three.

Note that `sl_proto::Object` *also* carries `sound` / `gain` / `sound_flags` /
`sound_radius` in its update block, and a fixture prim could set them today —
but the viewer does not read them: `world_sounds` is driven only by the
session events above. Whether the viewer *should* also honour the object-update
fields (the reference viewer does, in `LLViewerObject::processUpdateMessage`)
is a second question this task should answer rather than assume.

Wanted:

- a `SimSession` sender for the server-sent `SoundTrigger`, `AttachedSound`
  and `AttachedSoundGainChange`;
- a fixture shape for "this object loops this sound" — the catalogue's natural
  new prim is a `sound-box` looping `marker_tone(tones::MID)`, which costs no
  render baseline because a sound is invisible;
- an end-to-end test asserting the client raises `Event::AttachedSound` for
  it, and a `PreloadSound` on arrival if that is what a real region does.

This is the missing half of the sound path: the encoder proves the bytes are
real, and this proves they reach a voice. [[test-fake-grid-builtin-sounds]] is
about a different gap — the built-in *library* ids the viewer asks for on
arrival — and neither blocks the other.

Acceptance: a fixture object loops a fixture sound; a client against the stock
catalogue raises the attached-sound event for it and can fetch the clip it
names.
