---
id: gridspec-notecards-gestures
title: Notecards, embedded items and gestures on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, test-asset-save-mutation-survey, viewer-gesture-runtime]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim stores a new notecard as one zero byte, SL a valid empty text; OpenSim
flips gesture item flags, SL only reports active gestures at login.

## Discover

`notecard_create_update`, `gestures`, `CopyInventoryFromNotecard`, embedded-item
saves, the login gestures array after activation.

## Document

`book/src/gridspec/inventory.md` § Notecards and gestures.

## Fake grid

Small — placeholder body, copy-from-notecard, gesture flags / login array per
flavour.

## Viewer

Restore active gestures per grid.
