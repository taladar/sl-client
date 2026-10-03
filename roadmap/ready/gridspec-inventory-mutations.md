---
id: gridspec-inventory-mutations
title: Inventory item and folder operations and their pushes on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-inventory-service]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

Purge / remove are Trash-gated; OpenSim UDP mutations get no reply; SL rejects
UDP links into the COF; the fake grid drops about eight UDP mutation messages.

## Discover

`ais3-folder-lifecycle`, `inventory-item-ops` on aditi, recording the reply /
push shape per op; server-pushed removals after an object give.

## Document

`book/src/gridspec/inventory.md` § Changing inventory.

## Fake grid

Large — [[server-fake-grid-inventory-udp-mutations]].

## Viewer

AIS on SL, UDP on OpenSim; check SL accepts the UDP ops the viewer still sends.
