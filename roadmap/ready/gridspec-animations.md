---
id: gridspec-animations
title: Animations as each grid broadcasts them
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Play / stop complete on both grids with no divergence; the fake grid does not
decode `AgentAnimation`.

## Discover

`animation_play_stop` plus a two-avatar observer leg; server-added built-ins;
`AnimationSourceList`.

## Document

`book/src/gridspec/avatars.md` § Animations.

## Fake grid

Small in this task: decode, per-agent set, echo; the observer broadcast via
[[server-fake-grid-agent-avatars-shared]].

## Viewer

Stop as drop-out.
