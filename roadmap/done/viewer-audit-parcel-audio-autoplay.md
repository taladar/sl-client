---
id: viewer-audit-parcel-audio-autoplay
title: A transient empty parcel resets the user's stop decision and re-autoplays
topic: viewer
status: done
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-audio/src/parcel_audio.rs:404` —
`if parcel_url != audio.parcel_url { ... user_stopped = false; ... }`.

Any moment where `SlAgentParcel.current` or its `music_url` is momentarily
absent — a region crossing, a partial parcel update — flips the URL to `None`
(stopping the stream) and then back, which clears `user_stopped` and
**re-autoplays a stream the user explicitly stopped**. This is the
`update_parcel` clobber path.

Fix: treat a `None` parcel as "unknown", not as "a different parcel"; only clear
`user_stopped` on a genuinely different non-empty URL.

`parcel_audio.rs` has zero tests. Extract the decision as a pure function —
`(previous_url, new_url, enabled, user_stopped, running) -> Action::{Play(url),
Stop, Nothing}` — and assert: a same-URL re-delivery is `Nothing`;
`user_stopped` survives a same-URL update and a `None` gap but clears on a real
change; `enabled == false` never autoplays but an explicit play still works.

## Done (2026-09-15)

The policy moved out of `drive_parcel_audio` into a pure `AutoplayState` with
three transitions — `observe_parcel`, `observe_enabled`, `toggle_play` — each
returning a `StreamAction::{Play(url), Stop, Nothing}` the system hands to the
player. No player and no ECS in it, so it is tested directly.

Two changes of substance, not just a move:

- **The parcel is a three-state fact, not an `Option`.** `ParcelStream::Unknown`
  (no `SlAgentParcel`, or `current == None`) is distinguished from
  `Resolved(None)` (a parcel that really has no music). `Unknown` returns
  `Nothing` for everything — it neither stops the stream nor touches the stored
  URL — so a region crossing no longer interrupts the radio. `Resolved(None)`
  still stops, as it should.
- **The stop decision is a URL, not a flag.** `user_stopped: bool` became
  `stopped_url: Option<Url>`, and autoplay is armed when
  `stopped_url != parcel_url_raw`. There is now nothing for a gap to clear, and
  the module's documented promise ("stop remembers the choice *for that URL*")
  is literally what the code compares. Walking off a stopped parcel and back
  leaves it stopped; a genuinely different stream still re-arms autoplay.

Nine tests in `parcel_audio.rs` (the module had none): same-URL re-delivery,
the unresolved gap holding both stream and decision, a different stream
re-arming after a stop, a silent parcel stopping without clearing the decision,
a disallowed scheme reading as no stream, autoplay-off vs. the explicit play
button, the first sight of the setting starting nothing, the setting's
off/on round trip honouring a stop, and a missing agent parcel reading as
`Unknown`.
