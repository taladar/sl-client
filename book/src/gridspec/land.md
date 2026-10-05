# Land

What each grid does with a viewer's parcel traffic: how a parcel's record
arrives and what it carries, and what the grid answers when the About Land
floater, the land tool and the objects panel change it. The messages
themselves are described in
[Region & Estate Information](../content/region.md).

Measured on 2026-10-05 on Second Life's beta grid (aditi) and the local
OpenSim standalone by the conformance cases `parcel-properties`,
`parcel-edit`, `parcel-edit-refused`, `parcel-divide-join` and
`parcel-object-owners`. Each case holds both grids, and both fake-grid
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
