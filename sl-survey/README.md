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
4. To move to the next region it **logs in directly there** (using the name from
   the map, via `start=uri:Region&x&y&z`), surveys it, and repeats until the
   queue drains, `--max-regions` is reached, or it leaves the bounds.

### Why direct re-login instead of teleport

The library implements in-world teleport (`TeleportLocationRequest` + a
circuit-handover state machine), but a real cross-region teleport requires the
viewer to already hold a **child-agent UDP circuit** to the destination region
(opened from the `EnableSimulator` / `EstablishAgentCommunication` the source
region sends), so the destination has the agent's presence when the teleport
runs. This single-circuit client does not maintain child-agent circuits, so a
teleport times out ("could not establish connection to destination"). The survey
therefore visits each region by logging in directly — fast and reliable given
the map provides the names. Teleport remains as a fallback for any queued region
whose name is unknown. (Implementing child-agent circuits would make true
teleport work and avoid the per-region re-login.)

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
