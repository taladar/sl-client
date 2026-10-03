---
id: gridspec-estate
title: Estate info, access, covenant and estate actions on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-region-restart-schedule,
  test-kick-user, gridspec-aditi-test-land]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim: silent refusal without rights, four trailing access lists, deferred
deltas; kick text "You have been kicked out". Restart notices differ (SL
named `RegionRestart*` alerts, OpenSim plain text) and our code handles
neither. The fake grid answers none of the estate actions.

## Discover

Estate cases as OpenSim estate owner with two avatars (kick, eject, freeze,
teleport home, estate message, restart); aditi covenant leg (no rights needed);
restart notices observed opportunistically.

## Document

`book/src/gridspec/estate.md`.

## Fake grid

Small restart alert row in this task; actions —
[[server-fake-grid-estate-actions]].

## Viewer

One countdown from both restart forms; kicked / frozen / ejected handling.
