---
id: test-fake-grid-default-wearable-textures
title: Serve the default wearable textures so the own avatar is not untextured
topic: test
status: ready
origin: noticed live-verifying test-fake-grid-npc-avatars (2026-09-01)
points: 2
refs: [test-fake-grid-npc-avatars]
---

Context: [context/testing.md](../context/testing.md).

Logging the viewer into the fake grid, the **own** avatar's texture
fetches fail — one `asset not found` per default wearable texture
(`822ded49-…`, `11b4c57c-…`, `12149143-…`, `32bfbcea-…`, `d07f6eed-…`,
`3c59f7fe-…`, `1dc1368f-…`), each then burning the full retry budget
(`scheduling retry 1/6`, six times) before it gives up.

These are the reference viewer's built-in default body / clothing
textures, which a real grid serves from its library. The fake grid serves
no asset it was not handed, so the arriving avatar has nothing to paint
itself with, and the log noise buries anything else during arrival.

[[test-fake-grid-npc-avatars]] solved the same problem for *other*
avatars — `NpcAppearance::solid` names bakes and
`RegionFixture::into_scenario` registers their bytes — so the shape of the
fix is known: register a procedural solid (or a recognisable checker) for
each default wearable texture id in `scenario::default_assets`, next to
the four terrain detail solids that are already there.

Acceptance: an arrival against the stock scenario logs no
`asset not found`; the own avatar renders painted rather than untextured.
