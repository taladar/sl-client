---
id: gridspec-landmarks-home
title: Landmarks and the home location on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-places-landmarks]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Firestorm sets home via the `HomeLocation` cap first (OpenSim serves it), our
client only UDP; the fake grid mints empty landmark bodies and copies OpenSim's
Set-Home rule.

## Discover

An `sl-repl` probe / case: set home (permission rule, alert text, cap reply),
create a landmark and read its body; both grids.

## Document

`book/src/gridspec/teleport.md` § Home and landmarks.

## Fake grid

Small — landmark bodies, the cap, a Set-Home flavour row.

## Viewer

Cap first, UDP fallback.

## Capabilities done in this task

[[protocol-cap-home-location]]: setting home over `HomeLocation`, with the
reply each grid sends.

## From gridspec-teleport (2026-10-07)

Left for this task: the progress lines of a *successful* landmark teleport on
Second Life (`sending_landmark` was seen ahead of a refusal; the fake grid's
Second Life flavour assumes `resolving` and `Sending to destination.` follow,
as they do for home), and Second Life's refusal of a home teleport by an agent
with no home (OpenSim says `Home set not`; the fake grid's Second Life flavour
still sends `invalid_tport`). Both are rows of
`ImitatedGrid::teleport_policy`.
