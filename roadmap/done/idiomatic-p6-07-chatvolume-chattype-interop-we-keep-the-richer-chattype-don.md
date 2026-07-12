---
id: idiomatic-p6-07
title: ChatVolume ⇄ ChatType interop (we keep the richer ChatType, don't adop
topic: idiomatic
status: done
origin: IDIOMATIC_ROADMAP.md — Phase 6 — Adopt `sl-types` non-key value types (low-medium)
---

Context: [context/idiomatic.md](../context/idiomatic.md).

`ChatVolume` ⇄ `ChatType` interop (we keep the richer `ChatType`, don't
adopt `ChatVolume`). Implemented in `sl-proto/src/types/chat.rs`
(orphan-rule-legal — `ChatType` is local): `impl
From<sl_types::chat::ChatVolume> for ChatType` (total, lossless widening:
`Whisper→Whisper`, `Say→Normal`, `Shout→Shout`, `RegionSay→Region`) and the
fallible inverse `impl TryFrom<ChatType> for sl_types::chat::ChatVolume`
(`Whisper→Whisper`, `Normal→Say`, `Shout→Shout`, `Region→RegionSay`; every
non-volume type — the typing triggers, debug channel, owner, direct, and
`Unknown(_)` — yields the new public `ChatTypeNotAVolume { chat_type }` error,
modelled on `NegativeBalanceError`: `thiserror`, `#[non_exhaustive]`).
`ChatTypeNotAVolume` re-exported through `sl-proto`/`sl-client-tokio`/
`sl-client-bevy` (parity). +2 unit tests (the four volumes round-trip
`ChatVolume → ChatType → ChatVolume` identically; the six non-volume types
each narrow to `ChatTypeNotAVolume`). Consume-only — NO `sl-types` change. No
downstream/book change (a pure conversion API, no wire field). **Phase 6
COMPLETE.**
