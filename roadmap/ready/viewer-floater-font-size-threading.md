---
id: viewer-floater-font-size-threading
title: Part of several floaters is drawn at a fixed size whatever font size they are built at
topic: viewer
status: ready
origin: viewer-gallery-floaters-are-mostly-stubs (2026-09-25)
points: 3
refs: [viewer-gallery-floaters-are-mostly-stubs]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

Every floater's content builder now takes a `font_size` (the live window
passes its constant, the specimen the sweep cell's size), but in some windows
it does not reach every node, so the sweep's 11 / 15 / 22 px axis only partly
tests them:

- **Tables** keep their `TableSpec::font_size`, which is `'static`, so the
  cells of every table-bearing window (experience picker, search, the RLV
  lists, asset blacklist, avatar render settings, my environments, the
  settings picker, top scripts / colliders, telehub, About Land / Region)
  stay at 13 px in every cell.
- **Environment rows**: the shared spawners in
  `sl-viewer-environment/src/rows.rs` (also used by
  `sl-viewer-preferences/src/phototools.rs`) use a fixed 13 px.
- **Avatar and group profiles**: only the tab labels and a few headings
  follow; about twenty small helpers in each file keep 14 px / 13 px.

## What to do

Thread the size through. For tables, a per-spawn font size (a `TableSpec`
field that is not `'static`, or a `spawn_table` argument) is the one change
that fixes every window at once.

## Done when

At the sweep's 22 px cell every text node in these windows is at 22 px (or a
deliberate step from it), and the sweep passes.
