# sl-survey

Headless Second Life / OpenSim survey client. It logs in, enumerates the regions
of a bounded rectangle of the grid via the world map, visits each one, and
writes one JSON-lines record per region describing its parcels and region
metadata.

## What it collects

Per region:

- region name, maturity rating (PG / Mature / Adult), and product type
  (Full / Homestead / Openspace, where the grid reports it);
- region-wide agent and object/Land-Impact limits (`max_agents`,
  `hard_max_agents`, `hard_max_objects` from `RegionInfo`);
- per-parcel geometry (bounding box, area), rez-zone flags (object create /
  group create), access flags (ban list = banlines, access list, deny
  anonymous), and parcel/region prim limits;
- the in-bounds neighbouring region handles it discovered.

## How it works

1. Logs in at the start region (`--start-x`/`--start-y` give its grid
   coordinates, since the initial region handle is not surfaced by the
   protocol).
2. On arrival it requests the **world map** (`MapBlockRequest`) for the bounds,
   which returns every in-bounds region's name, coordinates, and maturity; these
   seed the visit queue and a handle→name table.
3. In each region it requests the region info and walks the parcel grid
   (`ParcelPropertiesRequest`) using the returned coverage bitmaps. The parcel
   replies (`ParcelProperties`) arrive over the CAPS event queue (the driver
   POSTs the seed capability, then long-polls `EventQueueGet` and feeds the LLSD
   events back into the session), since the simulator no longer sends them over
   UDP.
4. To move to the next region it **teleports** there
   (`TeleportLocationRequest`), surveys it, and repeats until the queue drains,
   `--max-regions` is reached, or it leaves the bounds. The whole survey runs on
   a single login.

### Teleport and the re-login fallback

Teleport works to any region by handle. The destination's address is delivered
by the source region as a `TeleportFinish` event over the CAPS event queue (not
UDP); on it the session hands its circuit over to the destination
(`UseCircuitCode` + `CompleteAgentMovement`), which the destination's presence
wait requires. If a teleport fails, the survey falls back to logging in directly
at the region (using its name from the map, via `start=uri:Region&x&y&z`) and
continues from there.

Because the initial region's handle is not surfaced by the protocol, give the
start region's grid coordinates with `--start-x`/`--start-y`, and a matching
`--start` location (e.g. `--start "uri:Region Name&128&128&30"`) so the first
record is labelled correctly.

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
  partially handled (parcel enumeration over-scans harmlessly). The map does
  report each region's true size, which a future version could use.
- Discovery is bounded by `--min-x/--min-y/--max-x/--max-y`; the world map
  covers the whole rectangle (including regions separated by empty grid space),
  unlike pure neighbour discovery.
