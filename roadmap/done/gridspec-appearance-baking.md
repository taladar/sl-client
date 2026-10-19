---
id: gridspec-appearance-baking
title: Appearance and baking on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-bake-service, viewer-bake-publish-morph-mask,
  viewer-bake-cache-query, server-fake-grid-agent-avatars-shared]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Aditi: `UpdateAvatarAppearance` resolves in two attempts, no
`AgentCachedTextureResponse`; OpenSim: no such cap, cached texture answered. The
fake grid accepts any COF version.

## Discover

The appearance cases; `RegionProtocols` bits on aditi; appearance-service URL;
self appearance after a bake; two-avatar observer leg on OpenSim.

## Document

`book/src/gridspec/appearance.md` § Baking.

## Fake grid

Small in this task: stale-version rejection, cached-texture reply. Real baking:
[[server-bake-service]].

## Viewer

Bake road per avatar, the COF-version retry.

## Done (2026-10-19)

The `appearance-baking` census (2av, all four grids; B's column live only)
records the login's appearance service and COF version, the central-bake bit
and the three appearance capabilities, the appearances A and B are sent on
arrival, a fetch from the appearance service, the baked-texture cache with
nil, made-up and published ids, the `UpdateAvatarAppearance` handshake raw
(version 0, the expected version, the same again, five ahead, our client's
request, `IncrementCOFVersion` and a bake at its answer), an
`AgentSetAppearance` of plywood and the arrival appearance published back
(`book/src/gridspec/appearance.md` § Baking). Findings:

- Second Life never answers `AgentCachedTexture` and ignores
  `AgentSetAppearance`; OpenSim answers every query, one entry per slot, a
  hit only for a cache id a publish named, and shows a publish to every
  other viewer but never to the publisher — and persists it, so the census
  publishes the arrival appearance back.
- Second Life refuses a bake of any version but the Current Outfit Folder's
  with a `400` naming the expected one, accepts the current version again
  and again, and states each accepted bake to the agent and to B at that
  version. A first run's "no answer" was a kept-alive connection the grid
  had closed.
- Both grids send an `AppearanceHover` block; neither sent a
  `RebakeAvatarTextures`. The appearance service answers a texture it holds
  no bake under `404` with "Hash mismatch".

Fake grid: small, done. `SimSession` decodes `AgentCachedTexture`
(`ServerEvent::CachedTexturesRequested`) and `AgentSetAppearance`
(`ServerEvent::AppearanceSet`) and can send `AgentCachedTextureResponse`;
its `UpdateAvatarAppearance` holds a request to the COF version as Second
Life does. `sl-fake-grid` `bakes.rs` answers the cache and keeps a publish
per `BakePolicy`, an accepted bake re-sends the agent's appearance at the
version baked, every own appearance carries the COF version and a hover
block, a client-baking arrival no longer sends the agent its appearance
twice, and the appearance service answers an unknown texture as Second Life
does. `appearance-baking` runs offline on both flavours; the observer
broadcast goes to [[server-fake-grid-agent-avatars-shared]]. Real baking
stays [[server-bake-service]].

Viewer: the server-bake driver already retries at the expected version (now
held by a unit test, `a_version_mismatch_is_retried_at_the_expected_version`)
and only client-bakes where `UploadBakedTexture` is granted. It never asks a
client-baking region's cache before re-uploading:
[[viewer-bake-cache-query]].

## Incomplete without server-bake-service

The fake grid's Second Life flavour answers the bake handshake's *shape*
(status, `success`, `error`, the versions, the core key set) but not its
*payload*, because it does not bake:

- an accepted bake's `textures` (45 entries), `visual_params` (253) and
  `avatar_scale` are absent, and the `AvatarAppearance` that follows names the
  fixture's solid placeholder bakes, not a composite of the outfit;
- a refusal's `cof_raw` and `debug_cof` (per-slot bake hash and layer texture
  ids, the worn wearables, the parameters, joint offsets, the attachments'
  render cost) are absent.

The census holds `*_core_keys` (the key set less those payload keys) and
leaves `*_keys` unheld until [[server-bake-service]] composites the bakes from
the Current Outfit Folder; that task then holds `*_keys` on `FakeSl` too.
What the payload entries hold, and how they relate to the `AvatarAppearance`
that follows, was not recorded — only their counts.
