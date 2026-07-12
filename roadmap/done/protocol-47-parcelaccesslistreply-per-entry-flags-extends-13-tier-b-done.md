---
id: protocol-47
title: ParcelAccessListReply per-entry flags (extends #13, Tier B). Done
topic: protocol
status: done
origin: ROADMAP.md — Tier E
---

Context: [context/protocol.md](../context/protocol.md).

**47. `ParcelAccessListReply` per-entry flags (extends #13, Tier B). ✅ Done.**
Each `List` entry of a `ParcelAccessListReply` (`session.rs`) carries `ID`,
`Time`, **and `Flags`**, but only id and time were mapped into
`ParcelAccessEntry` — the per-entry `AL_*` flags (the access/ban classification,
plus the Second Life experience allow/block sub-types) were dropped. Added a
`ParcelAccessFlags(u32)` bitfield value type (`ACCESS`/`BAN`/`ALLOW_EXPERIENCE`/
`BLOCK_EXPERIENCE`, with `union`/`contains`/`is_empty`, mirroring Firestorm's
`llparcelflags.h` `AL_*` constants) and a `flags` field on `ParcelAccessEntry`.
The reply handler now decodes each entry's wire `Flags` into it. The *update*
(send) path OR's any per-entry `ParcelAccessFlags` onto the list-level
`ParcelAccessScope` (so existing callers that leave it `NONE` still send just
the scope, while a Second Life client can flag an entry as an experience
allow/block) — matching OpenSim, whose `SendLandAccessListData`
(`LLClientView.cs` ~6651) sets each entry's `Flags` equal to the list's access
flag. Re-exported through both runtimes. Covered by the two existing
`lifecycle.rs` parcel-access tests, extended to assert the per-entry decode (a
`AL_BAN | AL_BLOCK_EXPERIENCE` entry) and the OR-onto-scope encode (an
`AL_ALLOW_EXPERIENCE` entry sent on an `AL_ACCESS` list). *Test: local OpenSim
(the existing #13 access-list round-trip already exercises the path; OpenSim
echoes the scope as the per-entry flags, so the experience sub-types are
unit-tested only — they need the SL grid).*
