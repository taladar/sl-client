---
id: gridspec-attachments
title: Attachments on each grid: attach, detach, limits, HUDs, temporary
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-remove-all-attachments]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim stamps `AttachItemID` and kills on detach; `RemoveAttachment` ignored on
live grids; the fake grid drops every attach / detach message.

## Discover

`attach_detach` on aditi plus multi-attach, HUD (second avatar must not see it),
drop, temp attach (scripted object), limits.

## Document

`book/src/gridspec/appearance.md` § Attachments.

## Fake grid

Large — [[fake-grid-own-attachments-and-region-moves]] and
[[server-world-sit-and-attach]].

## Viewer

Worn only after arrival; new local id per region.
