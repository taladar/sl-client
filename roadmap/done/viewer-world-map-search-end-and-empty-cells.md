---
id: viewer-world-map-search-end-and-empty-cells
title: World map — say when a search found nothing, and when a clicked cell is empty
topic: viewer
status: done
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

## Fixed (2026-11-11)

Done the reference viewer's way (`llfloaterworldmap.cpp`,
`llworldmapmessage.cpp`, `llworldmap.cpp`):

- **Search on commit.** The map searches on `Enter` in its field or a new
  **Find** button beside it (the reference's `DoSearch`), never as the field
  is typed in, so OpenSim's modal alert comes once, for a search that was
  asked for. The three-character floor is gone with the debounce: a short
  search is sent too. Second Life runs it, and OpenSim answers with its own
  "at least 3 characters" alert, which is the right thing to show for a
  search that was asked for.
- **"None found."** The rows are the known regions matching the search that
  was sent. When the reply's closing entry for that text arrives
  (`MapBlockKind::Terminator`, compared ignoring case) and nothing matches,
  the list shows "None found." (`worldmap_results_none_found`). A newer
  search clears it, and a late ending for an older search does not bring it
  back.
- **Empty cells.** A click into a cell the map knows no region in sends
  `MapBlockRequest` for that one cell with `RETURN_NULL_SIMS`. Until the grid
  answers, nothing is tracked and the location line reads "Loading…". A
  nameless entry, or a region with `Access` 254 (down), reads "Invalid
  location": it is not tracked, a double-click does not teleport there and
  the Teleport button does nothing. A region is tracked at the clicked spot,
  and gone to if the click was a double-click (on the same cell, while the
  check was still outstanding).

Tests: unit tests in `world_map.rs`
(`a_reply_says_whether_a_clicked_cell_is_a_place`,
`a_search_runs_when_asked_and_says_when_it_found_nothing`,
`a_double_clicked_unknown_cell_is_gone_to_only_once_the_grid_names_a_region`).
`e2e_pilot`'s world-map test on both fake flavours clicks the map's centre (its
own region), pans east until a click reads "Invalid location", and ends with a
search no region matches, which reads "None found.". The live stage
`the_live_world_map_knows_the_region_and_who_is_in_it` does the same on OpenSim
(open water two pans east of Default Region) and aditi (one pan east of Mauve),
both passing on 2026-11-11. Every e2e that searches the map now presses `Enter`
after typing.
