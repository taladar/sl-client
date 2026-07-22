---
id: viewer-sound-explorer
title: Sound explorer — nearby sound sources
topic: viewer
status: blocked
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

Deps: [[viewer-in-world-sounds]] (the source registry).
