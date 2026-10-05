# Land

What each grid does with a viewer's parcel traffic: how a parcel's record
arrives and what it carries, and what the grid answers when the About Land
floater, the land tool and the objects panel change it. The messages
themselves are described in
[Region & Estate Information](../content/region.md).

Measured on 2026-10-05 on Second Life's beta grid (aditi) and the local
OpenSim standalone by the conformance cases `parcel-properties`,
`parcel-crossing`, `parcel-edit`, `parcel-edit-refused`, `parcel-divide-join`
and `parcel-object-owners`. Each case holds both grids, and both fake-grid
flavours, to these answers as `Measured` constants. The aditi test accounts
own no land and there is no known way for a resident to get some there
(`gridspec-aditi-test-land`), so on Second Life only what a resident without
land rights sees is measured.

## Reading a parcel

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ParcelProperties` transport | event queue | event queue | event queue, both flavours |
| `MediaData` block (no media set) | sent: `none/none`, 0×0, loops | sent: `none/none`, 0×0, does not loop | as its grid |
| `MediaLinkSharing` block (no media set) | absent | absent; OpenSim has no such block | as its grid |
| `ParcelExtendedFlags` (`obscure_moap`) | sent | absent; OpenSim has no such block | as its grid |
| `SeeAVs` / `AnyAVSounds` / `GroupAVSounds` | sent | sent | both |
| `ParcelPropertiesRequestByID` | answered | **ignored** ("Unhandled packet … Ignoring") | `FakeOpensim` ignores it |

A viewer that refetches a parcel by its region-local id waits forever on
OpenSim. The rectangle form (`ParcelPropertiesRequest`) is answered on both
grids. The UDP `ParcelProperties` message has no room for the media and
extended-flag blocks, so a grid that sent it would lose them; neither does.

### Parcel properties: the fields

Read from the raw event (`RUST_LOG=sl_client_tokio::caps=trace` logs each
event-queue body before it is decoded). Both grids send the **same keys** in
the same five blocks — `ParcelData` (52 keys), `MediaData`,
`AgeVerificationBlock`, `RegionAllowAccessBlock`, `ParcelEnvironmentBlock` —
and Second Life a sixth, `ParcelExtendedFlags`. Six fields differ in their
LLSD type:

| field | Second Life | OpenSim |
| --- | --- | --- |
| `ParcelData.AuctionID` | 4-byte binary | integer |
| `ParcelData.ClaimDate` | integer (`time_t`) | `date` (`2026-05-28T18:28:29Z`) |
| `ParcelData.MediaAutoScale` | integer 0 / 1 | boolean |
| `MediaData.MediaLoop` | integer 0 / 1 | boolean |
| `MediaData.ObscureMedia` | integer 0 / 1 | boolean |
| `MediaData.ObscureMusic` | integer 0 / 1 | boolean |

`ParcelFlags` and `ParcelExtendedFlags.Flags` are 4-byte big-endian binary on
both. The client reads either form of each (`parcel_info_from_llsd`), and each
fake flavour writes its grid's (`sl_proto::ParcelLlsdDialect`, set per flavour
through `ParcelPolicy::wire_types`).

Prim limits are the region's and the parcel's own, not a grid constant. The
aditi sandbox measured (Mauve, 9008 m²) reports `MaxPrims` 3094,
`SimWideMaxPrims` 44019 and `ParcelPrimBonus` 10; OpenSim's region-wide parcel
reports 15000 for both and a bonus of 1.

| what a region holds | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| Openspace | 1000 land impact, 15 agents | no products: every region is 15000 prims, 40 agents (`RegionInfo` defaults) | as its grid, by `RegionConfig::product` |
| Homestead | 7500 land impact, 20 agents | the same 15000 / 40 | as its grid |
| Full Region | one product name over a range: commonly 20000–30000 land impact, 33–44 agents on small mainland regions, up to 175 on event regions | the same 15000 / 40 | `FakeSl` 20000 and 40 (raisable to 100) unless the region sets its own (`RegionConfig::capacity`) |

The Second Life rows are the product limits as residents know them, not a
measurement. On the fake grid a region's budget is shared among its parcels by
area — `MaxPrims` the parcel's share, `SimWideMaxPrims` the owner's total over
the region — and `RegionInfo` reports the same budget and agent limits
(`ImitatedGrid::region_capacity`).

### Parcel properties: the pushes

A grid sends the parcel under the agent without being asked. `parcel-crossing`
walks the avatar over a parcel line and back and records what comes.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| on arrival | pushed, sequence 0 | pushed, sequence 0 | pushed, sequence 0, both flavours |
| on crossing a parcel line | pushed, 1.4 s into a 16 m flight | pushed, 1.0 s into an 8 m flight | not pushed: the fake grid simulates no movement (`server-fake-grid-parcel-on-movement`) |
| the push's sequence id | **counts up** for the session: 0 on arrival, then 1, 2, … | always 0 | always 0 (the count belongs with the movement push) |
| the push's result and snap | `Single`, no snap | `Single`, no snap | the same |
| crossing back | pushed again | pushed again | — |

Second Life's push ids are small positive numbers, so a client that numbers
its own `ParcelPropertiesRequest`s upward from 1 cannot tell the answer to its
question from the next parcel the agent walks onto. The reference viewer keeps
its own ids negative (`SELECTED_PARCEL_SEQ_ID` -10000 and below); so does this
viewer since 2026-10-05 — About Land counts down from -100000, the land tool
from -10000. The session finds the agent's parcel from its position and the
pushed parcels' bitmaps, not from the sequence id.

## Managing a parcel

### Saving About Land

Both grids grant the `ParcelPropertiesUpdate` capability. The client POSTs the
parcel as the reference viewer's `LLParcel::packMessage` writes it, and falls
back to the UDP `ParcelPropertiesUpdate` only where a region does not grant the
capability. The capability body carries what the UDP message cannot: the media
type, size and loop flag, the avatar-visibility flags and `obscure_moap`.

The fallback matters on OpenSim. The UDP message has no media type, OpenSim
stores the missing field as `NULL`, and its SQLite store declares
`land.MediaType NOT NULL` — so after one UDP edit **every** later commit of the
region fails: the periodic land save, rezzing, and attaching. The
OpenSim-flavoured fake grid empties the media type on a UDP edit, so a client
that falls back where the capability is granted fails the fake grid's
`an_about_land_save_keeps_the_media_type_on_opensim`.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| accepted edit, the POST | not measured | answered | answered |
| accepted edit, the parcel pushed back | not measured | yes, under the sequence id of the client's **last** parcel request (`m_lastSeqId`) | `FakeOpensim` the same; `FakeSl` under `SELECTED_PARCEL_SEQ_ID` |
| refused edit, the POST | answered (no HTTP error) | answered (no HTTP error) | — (the fake grid enforces no land rights) |
| refused edit, an alert | none | none | — |
| refused edit, the parcel pushed back | yes, unchanged, under `-10000` (the reference's `SELECTED_PARCEL_SEQ_ID`) | no | — |

Neither grid tells a viewer its edit was refused. The only sign is that the
record comes back unchanged (Second Life) or does not come back at all
(OpenSim). The update's `flags` ask for the push; the client sends no refetch
after an edit, and the About Land floater merges whatever arrives for its
parcel by its region-local id.

### Dividing and joining

On OpenSim neither `ParcelDivide` nor `ParcelJoin` is answered: the region
re-sends the parcel overlay and nothing else. The land tool therefore asks for
the selected rectangle again after sending either, so the selection describes
the new parcel rather than the one that was split or merged. Second Life is not
measured. The fake grid has no divide or join yet
(`server-fake-grid-parcel-divide-join`).

### The objects panel

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ParcelObjectOwnersReply` transport | event queue (`UDPDeprecated` in the template; not measured) | UDP | as its grid |
| a return addressed to a parcel | not measured | by class (`RT_OWNER`, `RT_GROUP`, `RT_OTHER`) or by the owners named (`RT_LIST`, measured); the task list is **ignored** (`LandObject.ReturnLandObjects`) | `FakeOpensim` the same; `FakeSl` also takes the named tasks |
| a return addressed to the whole region (`LocalID = -1`) | not measured | the named tasks only (the top-objects window's return; from the source, `ReturnObjectsInParcel`) | both flavours |

A return on OpenSim cannot take one object: it takes everything its owner has
on the parcel. `parcel-object-owners` therefore rezzes and returns the object
of an account that owns nothing anywhere on the local grid, and tallies and
returns it as the land owner.
