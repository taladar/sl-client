---
id: gridspec-appearance-baking
title: Appearance and baking on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-bake-service, viewer-bake-publish-morph-mask]
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
