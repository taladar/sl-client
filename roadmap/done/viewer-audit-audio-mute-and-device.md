---
id: viewer-audit-audio-mute-and-device
title: Collision sounds ignore the object-sound mute exception, and a device fallback lies to the UI
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

- `sl-viewer-audio/src/world_sounds.rs:575` — `ingest_collisions` checks
  `mutes.is_muted(key.uuid())`, while the trigger and attached-sound paths
  (`:479`) go through `muted()` ->
  `is_muted_aspect(.., MuteFlags::ALLOW_OBJECT_SOUNDS)`. So a mute entry that
  **allows** object sounds still silences that object's collision sounds. Both
  paths should share one tested predicate.
- `sl-viewer-audio/src/audio.rs:238` — on a failed named-device open the mixer
  falls back to Default but records `*last = Some(stored)` (the *requested*
  name) and never corrects the setting, so the preferences combo keeps showing a
  device that is not in use, with only a `warn!`. It is never retried if the
  device reappears.

Two documentation contradictions in the same file worth resolving while there:
`world_sounds.rs:495` says "anything unrecognised map to plastic" while the call
site at `:584` says "unknown -> wood". Behaviour: an unknown material *byte*
gives plastic, a missing lookup gives `unwrap_or(3)` = wood. And
`collision_sound_str` (`:501`, `:587`) returns UUID **strings** that
`Uuid::parse_str` re-parses on every collision edge — that should be a const
table.

`audible_from_flags` (`:124`) is already pure and has no test for its
three-boolean truth table.

## What was done (2026-09-14)

### The mute exception

`ingest_collisions` now resolves **both** ids a mute entry can name — the
object's owner and the object itself — and tests them with the same `muted`
predicate the trigger and attached-sound paths use, through a thin
`collision_muted` wrapper that applies it to each side of the contact pair.
Before, it read only the object's own id and only through the bare `is_muted`,
so an entry whose "Block Object Sounds" toggle was *off* — the reference's
`LLMute::flagObjectSounds`, an entry that deliberately keeps letting that
source be heard — still silenced that object's collision sounds, and a mute that
named the **owner** rather than the object silenced nothing at all. Both halves
of that are fixed by going through one predicate.

Resolving the owner needed a new accessor:
`ObjectState::owner_and_key_by_scoped` returns `(AgentKey, ObjectKey)` in one
lookup. Deliberately a pair rather than two calls — both fields live on the
same `TrackedObject`, so either both are known or neither is, and an API that
can return a key without its owner would invite the caller to fall back to a nil
owner and quietly test the mute list for `00000000-…`.

### The device fallback

The fallback itself was right (the mixer's own automatic fallback only covers a
*running* device disappearing, so an explicit one is needed); what it did
afterwards was not. It recorded the requested name as though it had been applied
and stopped there.

**The lie started one layer lower than the audit said, and the first live run is
what found it.** With the viewer-side fix in place and a settings file naming a
device that does not exist, the log still read

```text
INFO firewheel_cpal: Starting output audio stream with device
  "Some(DeviceId(Alsa, "default"))"
```

and no note appeared — because `Mixer::start` never failed. It resolved
`DeviceSelection::Named(name)` through `find_output_device`, got `None`, and
passed that straight to cpal, where "no device id" *means* "the system default".
So the requested device was never opened, the default was opened instead, and
`Ok(())` came back: there was no error for the viewer's fallback branch to catch
and nothing above `sl-audio` could tell the difference. The `warn!` the audit
described could not fire at all.

`start` now returns `AudioError::NoDevice(name)` for a named device the host
does not enumerate — the variant already existed for exactly this ("the
requested device is gone") and had no constructor. Asking for the default is
what `DeviceSelection::Default` is for; a name that cannot be resolved is a
failure to honour the request, not a synonym for it.

The setting is deliberately **not** rewritten — an unplugged headset is expected
back, and demoting the user's choice to "system default" behind their back loses
it for good. Instead:

- a new `OutputDeviceStatus` resource carries the discrepancy (what the
  preference asks for, and whether it is actually playing);
- the named device is **retried** every `DEVICE_RETRY_SECONDS` (5 s) — but only
  once it is enumerable again (`device_retry`), so a device that comes back is
  picked up without a viewer restart, and one that does not costs a host
  enumeration rather than a failed stream open every five seconds;
- the preferences audio tab stops presenting the preference as the truth: the
  chosen device keeps its option in the combo even when the enumeration has lost
  it (otherwise the combo silently reads as "System default" and the choice
  looks forgotten), and a note under the combo says in words that it is not the
  device playing and will be used again when it returns.

The note needed a `spawn_pref_note` helper in the preferences shell — a
translated line of prose that annotates the rows around it. Like a section
heading and unlike a row, it carries no `PrefSearchRow`, so the search filter
leaves it alone.

### The two documentation contradictions

They were describing two genuinely different cases, which is why they read as a
contradiction. Both are now said precisely, and the distinction is asserted:

- an **unrecognised material byte** maps to plastic, as the reference's
  `sound_ids.cpp` table does;
- an **untracked object** has no material byte to look up at all, and falls back
  to the new named `DEFAULT_COLLISION_MATERIAL` — wood, the material a prim is
  created with — which is a different sound.

`collision_sound_str` became `collision_sound`: a `const fn` table of parsed
`Uuid`s (`uuid::uuid!`), so a collision edge no longer runs `Uuid::parse_str`,
and the unreachable parse-failure branch that used to skip the sound is gone.

## How it was verified

Unit tests, all client-side (the mute list, the material table and the device
decision are all pure):

- `world_sounds::tests::collision_mute_honours_the_object_sounds_exception` —
  the regression: a blanket mute on either side silences the collision, the same
  entry carrying `ALLOW_OBJECT_SOUNDS` does not, an owner mute silences that
  owner's collisions, and an untracked side contributes nothing. The
  object-sounds assertion fails against the old `is_muted` call.
- `world_sounds::tests::collision_sound_table` — the six named materials are
  distinct, LIGHT (7) and an unknown byte are plastic, and the untracked-object
  fallback is **not** that plastic.
- `world_sounds::tests::parcel_local_audibility_rule` — extended from four rows
  to the whole eight-row truth table.
- `mixer::tests::a_named_device_that_is_not_there_is_an_error` — the root of it:
  a named device the host does not have is `NoDevice`, and nothing is opened in
  its place. The check runs before any stream is opened, so the test touches no
  hardware.
- `audio::tests::device_retry_waits_for_the_device_to_reappear` — a device still
  absent is not re-opened; one that is back is.
- `preferences_audio::tests::an_unavailable_device_is_still_offered` and
  `the_unavailable_note_follows_the_status` — the combo keeps the choice, and
  the note appears and disappears with the status.

Live, against the local OpenSim grid, with a scratch `XDG_CONFIG_HOME` whose
`viewer-settings.toml` names an output device that does not exist (a scratch
config because the viewer rewrites its settings file on exit):

- **before the `sl-audio` fix** — no note, and the log showed cpal opening
  `DeviceId(Alsa, "default")` while reporting success. That run is what located
  the real defect; the viewer-side work alone would have shipped looking
  correct and doing nothing.
- **after it** — one
  `WARN sl_viewer_audio::audio: audio output device "…" could not be started
  (no audio output device available: …); falling back to the system default`,
  the combo still showing the missing device as the selection, the note under
  it, and the note going away when a working device is chosen.

Two things the log settles that the screen cannot. The retry did **not** spam: a
whole session on the fallback produced exactly one failed open, because the
five-second tick enumerates and finds the name absent rather than re-opening
blindly. And the settings file written when the first run exited still named the
unavailable device, so the user's choice survives a session spent on the
fallback.

Not exercised live: the device actually *coming back* — that wants a real
hot-plug. `device_retry`'s unit test covers the decision, and the
`info!("… is back; playing on it again")` it leads to is the only untried line.
