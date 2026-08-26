---
id: viewer-audit-parcel-audio-autoplay
title: A transient empty parcel resets the user's stop decision and re-autoplays
topic: viewer
status: bugs
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
