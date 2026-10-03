---
id: gridspec-avatar-presence
title: Other avatars in the region: full updates, coarse locations and kills
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-near-avatar-stuck-coarse-sphere,
  server-presence-service]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

The fake grid shows only NPC fixtures and never sends
`CoarseLocationUpdate`; on aditi a coarse-only avatar never got a full update
([[viewer-near-avatar-stuck-coarse-sphere]]).

## Discover

Two avatars: coarse update cadence and range, `you` / `prey` indices, the
interest range for full avatar updates, parcel privacy (`SeeAVs`) hiding,
kill-on-departure timing; radar / minimap via the two-viewer automation.

## Document

`book/src/gridspec/avatars.md`.

## Fake grid

Large — [[server-fake-grid-agent-avatars-shared]].

## Viewer

Promote a coarse dot to a body; hidden avatars; `e2e_two_avatars`.
