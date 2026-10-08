# World map

A viewer draws its world map from four UDP requests to the region it is in
and from a tile server. `MapBlockRequest` asks which regions lie in a
rectangle of grid cells and `MapNameRequest` which regions have a name; both
are answered with `MapBlockReply`. `MapItemRequest` asks for one kind of
marker — agents, telehubs, land for sale, events — and `MapLayerRequest` for
the images the map was once drawn from. The pictures themselves are JPEG
tiles fetched over HTTP from the `map-server-url` of the login response.

Measured on 2026-10-08 by the `map-blocks-items` conformance case, which
asks every request in every form from one resident's session while a second
resident stands by and then goes next door: three runs on aditi (in Ahern, a
mainland region with three neighbours) and three on the local OpenSim (the
south-western region of its 2×2 block, which has a telehub). The case
listens for a fixed time after each request, since no reply is one of the
answers. Rows marked **held** are checked on every run, live or offline:
each fake flavour against its live grid's answer.

Every reply of every kind came reliably, about 0.2 s after its request on
aditi and within 30 ms on OpenSim.

## Blocks

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what a region's entry states | its name, cell and rating. `RegionFlags`, `WaterHeight` and `Agents` are zero — with two residents in the region too — and there is no `Size` block | the same | the same |
| the map image id (**held**) | sent for a request with flags of zero, nil for one with the layer flag (2) | the same | the same |
| the flags a reply echoes (**held**) | the request's low sixteen bits: 2 for 2, 0 for the null-sims flag (`0x10000`), 2 for both | the same | the same |
| an empty cell asked about alone, without the null-sims flag (**held**) | no reply | no reply | no reply |
| the same with the flag (**held**) | one entry: the cell, no name, `Access` 255 | the same | the same |
| what else such an entry holds | whatever was in memory: region flags `0x0cbb1578` or the letters `enla`, water and agent counts to match; one entry in each larger answer had `Access` 254 and zeros instead | zeros | zeros |
| a rectangle of empty cells, with the flag (**held**) | an entry for each cell | no reply | each flavour's |
| a rectangle of regions and empty cells, with the flag (**held**) | the regions, then an entry for each empty cell | the regions alone | each flavour's |
| a rectangle with its bounds the wrong way round (**held**) | no reply | no reply | no reply |
| how large a rectangle is answered (**held** at 25 × 25) | up to 256 cells: 16 × 16 and 65 × 1 are, 18 × 18 and 400 × 1 are not, and get no reply at all | any: 400 × 1 and 25 × 25 are | 256 cells on `FakeSl`, any on `FakeOpensim` |
| how an answer is cut into replies | not seen cut: nine entries came in one | ten entries to a reply (its source; the local grid has four regions) | 255 on `FakeSl`, ten on `FakeOpensim` |

The reference viewer never asks for more than sixty-four regions at once
and calls that a limit the server enforces. A rectangle of 256 cells was
answered on aditi, where it held five regions; whether the limit counts
cells or regions found could not be told there. The reference viewer always
sends the layer flag, and so never sees a map image id; it asks about a
single clicked cell with the null-sims flag instead, to learn that nothing
is there.

## Name searches

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| how a reply ends (**held**) | with an entry at cell `(0, 0)` carrying the search text and `Access` 255 | the same | the same |
| a name with no match | that entry alone, and nothing else | that entry, and a modal `AgentAlertMessage`: `No regions found with that name.` | each flavour's (**held**) |
| what matches (**held**) | names beginning with the text: the name less its first letter finds nothing | names containing it: the name less its first letter finds the region | each flavour's |
| case (**held**) | ignored | ignored | ignored |
| a search of two letters (**held**) | run like any other | not run: the closing entry alone, and an `AlertMessage`: `Use a search string with at least 3 characters` | each flavour's |
| how many matches | not measured: a search with many would list other people's regions | twenty at most (its source) | all on `FakeSl`, twenty on `FakeOpensim` |

## Items

A `MapItemRequest` names an item type and a region; a handle of zero means
the region the agent is in. Every reply echoed flags of 2.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| telehubs (1) | the whole grid's, whichever region is named: eleven items in one reply, `Extra2` 0 for a telehub and 1 for an infohub, no id | handle zero: the agent's region's telehub, `Extra` and `Extra2` 0, in one reply. A region without one: no reply (its source) | no reply: it has none |
| land for sale (7) | the whole grid's, whichever region is named: 10,800 to 14,400 items in 490 to 650 replies of up to 27, the first seven at once and the rest over 16 to 28 s; the total differed from one request to the next. `Extra` the area, `Extra2` the price, the id set | handle zero: no reply (nothing was for sale) | no reply |
| adult land for sale (10) | the whole grid's: 435 items in 25 replies within 4 ms | no reply | no reply |
| events (2, 3, 9), classifieds (8), 4, 5 and 11 | no reply. Aditi may simply have no events | no reply | no reply |
| agent locations (6) for the agent's region | one reply with one item | the same | the same |
| a request naming the agent's own region, of any type (**held** for telehubs) | the type asked for | the region's agent locations, then its telehub, whatever type was asked for | each flavour's |
| a neighbouring region's agent locations | one reply, 0.27 s | one reply; its telehub and land for sale were not sent with it | one reply |
| a cell with no region (**held**) | no reply | no reply | no reply |

### Agent locations

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| whether the asker is counted | no: alone in a region its item has an `Extra` of 0 | no: "own position is not sent" | nobody is |
| a region with nobody to show (**held**) | one item on the region's south-west corner, `Extra` 0 | one item a metre in from it on both axes, `Extra` 0 | each flavour's |
| where an item is put | not where the avatar stands: two residents at (8, 10) were one item on the corner, `Extra` 1 to each of them; one at (128, 128) was an item at (128, 128). The step is somewhere from 11 to 128 m | where the avatar stands, cut to whole metres, one item for each with `Extra` 1 | the corner item alone |
| `Extra` | how many others are at that spot. The first run's counts were one higher throughout, which a third resident in the region would explain | 1 | 0 |
| `Name` | a UUID for another region; seven or eight characters that are not digits for the agent's own | 32 hexadecimal digits: an MD5 of the region's name and a clock tick | a UUID on `FakeSl`, 32 digits on `FakeOpensim` |
| how soon somebody leaving shows | in the first answer, 2 s later, in both regions | in its own region at once. Asked from another region, the answer is kept for two minutes: the neighbour went on reading as empty 75 s after somebody arrived there and showed them at 150 s | nobody leaves |
| a region with one resident, asked about from another | that resident | that resident, and the region's telehub with it, this time with an id | the corner item |

A viewer has to skip an item whose `Extra` is zero on both grids, and ours
does. On Second Life the items are a count at a coarse position and not a
dot for each avatar.

## Layers

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `MapLayerRequest` (**held**) | a reply with no layers, echoing flags of 2 | one layer from `(0, 0)` to `(30000, 30000)` with the image `00000000-0000-1111-9999-000000000006`, under flags of 0 | each flavour's |
| the `MapLayer` capability | not measured: our client does not ask its seed for it | not measured; granted when asked for (see [Capabilities](capabilities.md)) | not served |

## Tiles

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `map-server-url` in the login | `https://secondlife-maps-cdn-dev.akamaized.net/aditi/` | the login URI, `http://127.0.0.1:9000/` | its login URI |
| the region's tile, zoom 1 to 8 | `200`, `image/jpeg`, 256 × 256, from 11 kB at zoom 1 to 0.8 kB at zoom 8 | `200`, `image/jpeg`, 256 × 256, 4.9 kB down to 1.7 kB | zoom 1: the stock tile. Coarser: as an absent tile |
| its caching (**held**) | `Cache-Control: max-age=` up to 86,400, an `ETag` and a `Last-Modified` | none of the three; `Server: OSWebServer` | all three on `FakeSl`, none on `FakeOpensim` |
| zoom 9 | `403`, `application/xml`: an `AccessDenied` document | `200`: 1,653 bytes of plain water | as an absent tile |
| a tile of open ocean (**held**) | the same `403` | the same water tile | `403` with that document on `FakeSl`, OpenSim's water tile on `FakeOpensim` |
| a zoom-2 tile named by a region that is not its corner | the same `403` | the same water tile | as an absent tile |

## What our viewer does with it

- It asks for blocks in rectangles of eight by eight cells with the layer
  flag, as the reference viewer does. Before this measurement it asked for
  sixteen by sixteen with flags of zero, which is the largest rectangle
  Second Life answers.
- It asks for agent locations region by region and for every other item
  once, of "the region I am in". It used to ask for every type of every
  visible region, which on Second Life is the grid's whole list of land for
  sale — some twelve thousand items — for each of them. A reply of anything
  but agent locations is added to what is known, since one region's parcels
  arrive spread over hundreds of replies.
- Its search sends nothing shorter than three characters, so that typing
  does not raise OpenSim's alert, and lists a match anywhere in a name. The
  entry that ends a search is no region and makes no row.
  `Event::MapBlockBatch` carries each reply whole — that entry and the
  empty cells included — for a client that wants to know a search is over.
- A tile answered `403` is a failed fetch: it is tried again a few times
  and then left undrawn. OpenSim's water tile is drawn as any other.

Checked through the viewer automation, which reads what the map knows with
its `world_map` probe: on each fake flavour the map's search finds the
neighbouring region and its Teleport button goes there
(`e2e_pilot`); and with two viewers on each live grid
(`SL_E2E_GRID=opensim` and `=aditi`, `e2e_two_avatars`) the map names the
region it is opened in, draws a tile from the grid's tile server, holds an
agent location counting the other resident, and lists the region for a
search of the start of its name — on OpenSim for its end as well.

Still open: the modal alert OpenSim sends for a search that matches nothing
is raised on every such search as it is typed, and nothing in the map says
that a search has finished or that a clicked cell is empty
(`viewer-world-map-search-end-and-empty-cells`).
