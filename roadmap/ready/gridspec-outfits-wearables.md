---
id: gridspec-outfits-wearables
title: The Current Outfit Folder and wearables on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-bake-cof-layer-order, viewer-outfit-editor]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

COF green on both grids; SL rejects UDP links into it; `AgentWearablesUpdate`
full on OpenSim, a dummy on aditi; the fake grid ignores UDP COF links and
`AgentIsNowWearing`.

## Discover

`current_outfit_folder`, `wearables_request`, wearing / replacing outfits via
the live e2e; ordering conventions; unsolicited `AgentWearablesUpdate`.

## Document

`book/src/gridspec/appearance.md` § Outfits.

## Fake grid

Medium — [[fake-grid-own-attachments-and-region-moves]].

## Viewer

Outfit read from the COF on SL, wearables on OpenSim.
