---
id: test-set-appearance
title: publish appearance (AgentSetAppearance) and query the baked-texture ca
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 14 — Appearance, attachments & animations `[both]`
---

Context: [context/test.md](../context/test.md).

`set-appearance` — publish appearance (`AgentSetAppearance`) and query the
baked-texture cache (`AgentCachedTexture`): the two wire messages a viewer
uses to advertise a locally-composited avatar and skip re-uploading bakes
the grid already holds. `1av`. Two exchanges: (1) a baked-texture cache
query for the classic bakes (head / upper / lower / eyes / hair) whose
`AgentCachedTextureResponse` ([`Event::CachedTextureResponse`]) echoes the
serial with one `(slot, cached id)` entry per queried slot; (2) an
`AgentSetAppearance` publishing a full 45-face `TextureEntry` (baked slots
naming a real reference texture), a per-slot cache id, a neutral
visual-param set and the avatar bounding box. The command/event and the
`encode_texture_entry` packer already existed; net-new was re-exporting
`encode_texture_entry` from both runtimes plus the case. **Grid divergence**
(the mirror of `server-appearance-bake`, which is `partial` on OpenSim). On
OpenSim the legacy client-side bake is the live path, so the case re-queries
the cache after the publish and records the server-side ingestion signal (a
cache hit for the published id, or a `RebakeAvatarTextures` for it) as a
**metric** rather than an assertion — whether it fires depends on the
region's baked-cache internals (asset presence, and on the now-multi-region
local grid the login lands as a *child* presence), so a run is not failed
for its absence; the deterministic cache-query reply and the wire exchange
are what the case guarantees (green, `complete`). On modern Second Life
(aditi) central baking supersedes both messages: **live finding** — the
simulator does *not* answer the legacy `AgentCachedTexture` at all
(`cache_query_answered = 0`), so there the query is best-effort and the
publish a wire exercise, recorded `partial`. `[both]`.
