---
id: gridspec-world-map
title: World map blocks, items, layers and tiles on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-world-map-tracking-teleport,
  viewer-world-map-search-end-and-empty-cells,
  server-fake-grid-agent-avatars-shared, gridspec-seed-capabilities]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-08)

Measured and written up in `book/src/gridspec/world-map.md`.

- **Discover.** `map-blocks-items` rewritten to ask every map request in
  every form and listen a fixed time after each: block rectangles with each
  flag, of regions, of empty cells and of ten sizes; seven name searches;
  eleven item types about "here" and about a named region; the layer
  request; the tile server at every zoom. A second resident stands by and
  then goes next door, for the agent locations. Three runs on aditi, three
  on OpenSim.
- **Findings.** Second Life answers no block rectangle of more than 256
  cells, and reports every empty cell of a null-sims rectangle where OpenSim
  reports one only when asked about alone. Both end a name search with an
  entry at cell `(0, 0)` carrying the search text; Second Life matches the
  start of a name and OpenSim anywhere in it, refusing a search under three
  characters with an alert and a search with no match with a modal one.
  Second Life answers telehub and land-for-sale requests for the **whole
  grid** whichever region is named — some twelve thousand parcels in five
  to six hundred replies — and OpenSim answers a request naming the agent's
  own region with its agent locations whatever type was asked. Neither
  counts the asker among the agents; both send one item with a count of
  zero for a region with nobody to show; Second Life states counts at
  coarse positions, and OpenSim keeps another region's answer for two
  minutes. Second Life has no map layers; OpenSim has one over the whole
  grid. Second Life's tile server refuses an absent tile with `403`;
  OpenSim serves a tile of water.
- **Client.** `Event::MapBlockBatch` delivers each `MapBlockReply` whole
  (echoed flags, empty cells, the entry that ends a search), the item and
  layer events carry the reply's flags, `RequestMapBlocks` takes the
  request's flags, and `SimSession::send_map_block_batch` is the server-side
  inverse.
- **Fake grid.** `ImitatedGrid::map_policy`: each of the divergences above,
  and the tile server's absent tile and cache headers. `map-blocks-items`
  holds both flavours and both live grids to the answers the chapter
  marks **held**.
- **Viewer.** Blocks are asked for eight by eight cells with the layer
  flag, as the reference viewer does; every item but agent locations is
  asked for once, of "here", and its replies are added up instead of each
  replacing the last (it used to ask for the whole grid's land for sale once
  per visible region and keep the last reply's worth); the search sends
  nothing under three characters. The `e2e_pilot` world-map test runs on
  both flavours, with OpenSim's match inside a name. A new `world_map`
  automation probe (`sl-viewer-ctl world-map`) reads what the map knows —
  regions, item layers, tiles — since the map is one composited image; with
  it a new two-viewer test checks the map on each live grid
  (`the_live_world_map_knows_the_region_and_who_is_in_it`, run with
  `SL_E2E_GRID=opensim` and `=aditi`).
- **Not done here.** The `MapLayer` capability: our client does not ask its
  seed for it. Events and classifieds: nothing came back on aditi, which
  may have none. How Second Life cuts a long block answer into replies, and
  how many matches a search returns there. The size of the step Second Life
  rounds an agent's position to. Agent locations on the fake grid are the
  empty-region item only, until
  [[server-fake-grid-agent-avatars-shared]].
- **Filed.** [[viewer-world-map-search-end-and-empty-cells]].

## Known already

OpenSim: a placeholder green dot, one whole-grid layer. SL: one global
layer, tiles from a CDN. The aditi run of `map-blocks-items` was deferred.

## Discover

Run `map-blocks-items` on aditi; add per-item-type probes (telehub, land for
sale, events, agent counts), `MapNameRequest`, the tile host.

## Document

`book/src/gridspec/world-map.md`.

## Fake grid

Small — per-flavour item / layer answers in this task.

## Viewer

Tile URL per grid, placeholder items; the world map floater via automation on
both flavours.
