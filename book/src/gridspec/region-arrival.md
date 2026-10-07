# Region arrival

What a region says, unasked, to an agent that has just arrived in it: who it
is, in what order the arrival comes, and what it then goes on sending on a
timer. The messages themselves are described in
[Region & Estate Information](../content/region.md).

Measured on 2026-10-07 on Second Life's beta grid (aditi) and the local
OpenSim standalone (a 2×2 block of regions), by `region-arrival`: one login
with the circuits probed from the first datagram, watched for 32 seconds. It
ran on aditi from two regions on two simulator channels — the sandbox the
first test account stands in, which has no neighbours, and a mainland region
with five — and on OpenSim's `Default Region`. Two scripted `sl-repl` logins
per grid with the wire and capability traces on gave the fields no event
carries.

The case holds aditi, OpenSim and both fake flavours to the rows marked
**held**. Everything else is recorded: it is the region's own, and another
region answers differently.

## The handshake's identity

| field | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `RegionFlags` | `0x14108226` (sandbox), `0x101182a6` (mainland) | `0x14108026` | `0x14108026` on both |
| `RegionFlagsExtended` (`RegionInfo4`) | one block, equal to `RegionFlags` | the same | the same |
| `RegionProtocols` (**held**) | `1`: the central-bake bit | bit 63 alone ("more than six baked textures") | each flavour's |
| `SimAccess` | 21 (Mature) on both regions | 13 (PG) | the region's configured rating |
| `SimOwner` | the nil id on both mainland regions | the estate owner | the estate owner |
| `BillableFactor` | 1.0 (sandbox), 0.0 (mainland) | 0.0 | `FakeSl` 1.0, `FakeOpensim` 0.0 |
| `ProductName`, `ProductSKU` (**held**: named or not) | "Mainland / Full Region", "023" | both empty | `FakeSl` the same two for a stock region; `FakeOpensim` empty |
| `ColoName` (**held**: named or not) | `aws-us-west-2b`, `aws-us-west-2a` | empty | `FakeSl` `aws-us-west-2b`; `FakeOpensim` empty |
| `CPUClassID`, `CPURatio` | 1140, 1 | 9, 1 | each flavour's |
| `TerrainDetail0`–`3` | four nil ids (sandbox); four textures (mainland) | four textures | the region's configured textures |
| `TerrainBase0`–`3` | nil | nil | nil |
| `WaterHeight` | 20.0 | 20.0 | the region's |
| `RegionHandshake`s down a child circuit (**held**) | **two**, a tenth of a millisecond apart, on each of five | one, on each of three | each flavour's |

The flags both grids' stock regions share are: landmarks and set-home
allowed, direct teleport, parcel changes and voice allowed, externally
visible, and bit 5, which this client has no name for. The sandbox adds
bit 9.

An OpenSim region names no product, so `ProductType::classify` has nothing
to go on and the session reports `Unknown`: what OpenSim's regions are is
not something the handshake says.

`ColoName` was not surfaced before this measurement; it is
`RegionIdentity::colo_name` now.

## The order of the arrival

Both grids open the root circuit the same way (**held**):

1. `AgentDataUpdate` — the agent's name and active group;
2. `RegionHandshake`;
3. `AgentMovementComplete`, with the pose and the simulator's channel.

| what follows | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the channel in `AgentMovementComplete` | "Luau 2026-07-24.30110090421" (sandbox), "Riders Test Channel 2026-10-05.37349263929" (mainland) | "OpenSim 0.9.2.1 Yeti Dev   36f6d16 (Unix/Mono)" | `sl-fake-grid` and its version, on both |
| `HealthMessage` (**held**) | sent with the movement completion, health 100 | not sent | `FakeSl` only |
| `AgentStateUpdate` on the event queue (**held**) | pushed 0.2 s after the handshake: the navmesh rights and the agent's preferences | not sent | `FakeSl` only |
| `ScriptControlChange` | seen on the account that wears a scripted attachment | not seen | not sent |
| the neighbours' announcement (`EnableSimulator`, `EstablishAgentCommunication`) | 1.5 s and 2.6 s after the handshake, and **again** about every five seconds: six of each per neighbour in 32 s | once each, 0.9 s after the handshake | once each |
| `AgentGroupDataUpdate` | not seen in 35 s | on the event queue, with the handshake | not sent on arrival |
| `CameraConstraint` | not seen | once, 27 s in | not sent |

Second Life's repeated `EnableSimulator` is answered by the session as the
first was: a neighbour it already holds a circuit to is not opened again.
The handshake a child circuit receives twice is likewise taken twice.

## What the region goes on sending

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `SimStats` (**held**) | every 2.00–2.02 s | every 2.99–3.00 s | every 2 s on `FakeSl`, every 3 s on `FakeOpensim` |
| its statistic ids (**held**) | 35: 0–15, 17–20, 24–35, 38–40 | 41: every id from 0 to 40 | each flavour's set, in the grid's own order |
| its `RegionFlags` | the handshake's | **not** the handshake's: `0x14000006` against `0x14108026` | the handshake's, on both |
| its `ObjectCapacity` | 22,510 and 22,500 | 15,000 | the flavour's land-impact budget |
| `SimulatorViewerTimeMessage` (**held**) | every 10.0 s | every 2.53–2.55 s | every 10 s on `FakeSl`, every 2.55 s on `FakeOpensim` |
| its `SunDirection` (**held**: sent or not) | a unit vector | the zero vector | each flavour's |
| its `SunPhase` | 1.11–1.40, following the sun | 2.67–2.75, advancing | fixed |
| its `UsecSinceStart` | the UNIX time in microseconds | the same | the same |
| its `SecPerDay`, `SecPerYear` | 14,400 and 158,400 | the same | the same |
| the first of each after the handshake | the time message 1–9 s in, the statistics 1.7–2.7 s: each on the region's own cycle | 0.9 s and 1.9 s | a second in, both |
| `CoarseLocationUpdate` | every 1.33 s | every 4.54 s | not sent on a timer |
| any of it down a child circuit | none but the coarse locations | the same | none |

The six statistics OpenSim sends and Second Life does not are ids 16, 21,
22, 23, 36 and 37. No id above 40 appeared on either grid.

`UsecSinceStart` is, on both grids, the wall clock and not the time since the
simulator started.

## What the fake grid does with it

`ImitatedGrid::arrival_policy` carries each flavour's column: the handshake's
product, data centre, CPU class and billable factor, the second handshake down
a child circuit, the `HealthMessage` and the `AgentStateUpdate`, and the
region's periodic telemetry — `SimSession::set_region_telemetry`, which sends
a root agent `SimStats` and the time message at the two intervals given and
stops when the agent becomes a child. Both flavours now open an arrival with
the `AgentDataUpdate` the live grids send, and answer a request for another.

The statistics are an idle region's (time dilation 1, each grid's own frame
rate, the rest zero) and the sun does not move: a region clock that things
happen on is `server-world-heartbeat`.

A region configured as a Homestead or an Openspace keeps that product's name
on either flavour; only the stock Full Region takes the flavour's.

## What the viewer does with it

- The About window's support block shows a Product line when the handshake
  names one and none when it does not; the e2e tests
  `the_about_window_names_a_second_life_flavoured_regions_product` and
  `the_about_window_shows_no_product_on_an_opensim_flavoured_region`
  (`e2e_live_checks.rs`) hold it to each flavour.
- `region-arrival` runs against both fake flavours on every `cargo test`, so
  the session takes a doubled child handshake, a `HealthMessage` and an
  `AgentStateUpdate` on one and their absence on the other.
- Every viewer test on the fake grid now arrives the way a live login does:
  an `AgentDataUpdate` first, the region's flags set, the statistics and the
  time arriving on a timer.

## Not measured

| behaviour | why |
| --- | --- |
| how often Second Life repeats a neighbour's announcement, exactly | counted (six in 32 s), not timed |
| an estate (non-mainland) region's product, SKU and owner on Second Life | the test accounts stand on mainland |
| `RegionInfo5` in the handshake | the handshake has no such block; the chat ranges it carries belong to `RegionInfo` (`gridspec-region-info`) |
| what bit 5 of the region flags means | set on every region measured; not looked up |
| the arrival after a teleport or a crossing | left to those chapters |
