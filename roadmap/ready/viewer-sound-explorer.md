---
id: viewer-sound-explorer
title: Sound explorer — nearby sound sources
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-in-world-sounds]
refs: [viewer-derender-blacklist, viewer-block-list]
---

Context: [context/viewer.md](../context/viewer.md).

Firestorm's sound explorer: a live list of the in-world sounds playing /
recently played ([[viewer-in-world-sounds]] owns the sound engine whose
source registry this reads) — per row the sound asset, owning object,
object owner, position/distance — with the actions that make it the
anti-noise tool: beacon to the source, **blacklist the asset**
([[viewer-derender-blacklist]]'s asset list), **mute the owner or object**
([[viewer-block-list]]), and play-locally to identify a sound.

Reference (Firestorm, read-only): `NACLfloaterexploresounds`,
`floater_NACL_explore_sounds.xml`.

Note (2026-08-19): the blacklist half is **already built and honoured** —
[[viewer-derender-blacklist]] shipped `DerenderKind::Sound`, its per-account
persistence, its row in the Asset Blacklist floater, and the refusal itself
(`world_sounds.rs` drops a blacklisted asset's `SoundTrigger`, `AttachedSound`
and `PreloadSound`, as the reference does). This floater is the **producer**:
"Blacklist the asset" writes a `RequestDerender` with
`DerenderKind::Sound`, which is the only thing missing. The reference's
per-flag variants (blacklist an avatar's *worn* / *rezzed* / *gesture* sounds,
`FSAssetBlacklist::eBlacklistFlag`) are not modelled yet — they are an entry
flag plus a Flags column in that floater, and belong with this task since it
is the only surface that produces them.

Deps: [[viewer-in-world-sounds]] (the source registry).
