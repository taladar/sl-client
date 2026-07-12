---
id: protocol-22
title: Sound
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**22. Sound — `SoundTrigger`, `AttachedSound`, `PreloadSound`,
`AttachedSoundGainChange` · 3 pts. ✅ Done.** Receive and locate spatial sound
events; fetch the clips via #19. Sound is entirely sim-pushed (a scripted
`llTriggerSound`/`llPlaySound`/`llPreloadSound`), so this is a receive-only
surface with no commands. Four new events, each decoded in the main dispatch and
surfaced verbatim: **`Event::SoundTrigger`** (a one-shot spatial sound — sound /
owner / object ids, the triggering object's `parent_id` as `Option<Uuid>` with
the wire's nil → `None`, the sound's own `region_handle` since a trigger can
come from a neighbouring region, the region-local `position`, and `gain`);
**`Event::AttachedSound`** (a sound bound to an object — ids, `gain`, and a new
`SoundFlags` bitfield mirroring the viewer's `LL_SOUND_FLAG_*` constants:
`LOOP`/`SYNC_MASTER`/`SYNC_SLAVE`/`SYNC_PENDING`/`QUEUE`/`STOP`, with
`is_loop`/`is_stop`/`contains` helpers); **`Event::AttachedSoundGainChange`**
(object id + new gain, applying to the current attached sound);
**`Event::PreloadSound`** (a pre-fetch hint carrying a `Vec<SoundPreload>`, each
`{sound_id, object_id, owner_id}`). New value types `SoundFlags` and
`SoundPreload`, re-exported through both runtimes' lib re-exports (no
command/`SlCommand` variants — nothing to send). Covered by three `lifecycle.rs`
tests (the `SoundTrigger` decode incl. nil-parent → `None`, the `AttachedSound`
flag decode, and the multi-entry `PreloadSound` decode). *Live-verified against
the local OpenSim via the `tokio_login_hold_logout` example and a new
`slclient22.oar` (a scripted prim looping
`llTriggerSound`/`llPlaySound`/`llPreloadSound` of the built-in `UISndAlert`
sound `ed124764-…` on a 5 s timer): logging in next to the prim at
(128, 128, 30) the client received, every tick, an `Event::SoundTrigger`
(position (128,128,30),
gain 1), an `Event::AttachedSound` (gain 1, loop=false/stop=false), and an
`Event::PreloadSound` for that sound. `AttachedSoundGainChange` is the
`llSetSoundVolume`-on-an-already-playing-sound path and is unit-tested only.
Test: local OpenSim with the script engine enabled and a sound-playing scripted
object (same OAR mechanism as #8).*
