---
id: viewer-world-map-search-end-and-empty-cells
title: World map — say when a search found nothing, and when a clicked cell is empty
topic: viewer
status: bugs
origin: gridspec-world-map (2026-10-08)
refs: [gridspec-world-map, viewer-world-map-tracking-teleport]
---

Context: [context/viewer.md](../context/viewer.md).

Measured in `book/src/gridspec/world-map.md`; three things the world-map
floater does not do with it.

- **A search that matches nothing shows nothing.** Both grids end every
  `MapNameRequest` reply with an entry at cell `(0, 0)` carrying the search
  text, and `Event::MapBlockBatch` now delivers it
  (`MapBlockKind::Terminator`). The result list should say "none found" when
  the entry for the text in the field arrives with no region beside it, as
  the reference viewer's does.
- **OpenSim raises a modal alert for every such search.** It answers a
  search with no match with a modal `AgentAlertMessage` ("No regions found
  with that name."), and our field searches as it is typed in, so a name
  typed slowly raises one alert per pause. The reference viewer searches on
  Enter or its button only. Either search that way on a grid that alerts, or
  swallow that alert while the map's own "none found" says the same.
- **A click on an empty cell is not known to be empty.** The reference
  viewer asks about a clicked cell with
  `MapRequestFlags::RETURN_NULL_SIMS` and both grids answer with a nameless
  entry (`MapBlockKind::EmptyCell`); ours never asks, so a click or a
  double-click on open ocean selects, tracks and tries to teleport to a
  place that is not there.
