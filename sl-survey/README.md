# sl-survey

Headless Second Life / OpenSim survey client. It logs in, then teleports across
a bounded rectangle of the grid — discovering neighbouring regions as it goes —
and writes one JSON-lines record per region describing its parcels and region
metadata.

## What it collects

Per region:

- region name, maturity rating (PG / Mature / Adult), and product type
  (Full / Homestead / Openspace, where the grid reports it);
- region-wide agent and object/Land-Impact limits (where the grid provides
  them — some are estate-manager-only on Second Life);
- per-parcel geometry (bounding box, area), rez-zone flags (object create /
  group create), access flags (ban list = banlines, access list, deny
  anonymous), and parcel/region prim limits;
- the in-bounds neighbouring region handles it discovered.

## How it works

1. Logs in at the start region (`--start-x`/`--start-y` give its grid
   coordinates, since the initial region handle is not surfaced by the
   protocol; every later region's handle comes from the teleport response).
2. In each region it advertises a draw distance so the simulator enables the
   neighbouring regions (`EnableSimulator`), requests the region info, and walks
   the parcel grid (`ParcelProperties`) using the returned coverage bitmaps.
3. It performs a breadth-first traversal: queue in-bounds, unvisited neighbours
   and teleport to each in turn, until the queue empties, `--max-regions` is
   reached, or it leaves the bounds.

A teleport that fails is logged and skipped (the next queued region is tried).
Region-name-based re-login fallback is **not** implemented: neighbour discovery
yields region *handles*, not names, and the login `start=uri:Region&x&y&z`
form needs a region name — resolving handles to names requires the world-map
lookup, which is a planned future addition.

## Usage

```sh
SL_PASSWORD=secret sl-survey survey \
  --login-uri http://127.0.0.1:9000/ \
  --first Test --last User \
  --start-x 1000 --start-y 1000 \
  --min-x 1000 --min-y 1000 --max-x 1001 --max-y 1001 \
  --output regions.jsonl
```

Key options: `--start` (login location, default `last`), `--channel` /
`--version` (viewer identity), `--draw-distance` (metres, default 256),
`--collection-time` (per region, default `12s`), `--max-regions` (default 64),
`--output` (`-` for stdout).

The `generate-manpage` and `generate-shell-completion` subcommands emit a man
page and shell completions respectively.

## Caveats

- Region size is assumed to be 256 m; OpenSim variable-sized regions are only
  partially handled (parcel enumeration over-scans harmlessly).
- Neighbour discovery only reaches regions connected by adjacency within the
  bounds; clusters separated by empty grid space are not found (the world-map
  source would cover those).
