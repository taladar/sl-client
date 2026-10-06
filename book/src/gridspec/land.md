# Land

What each grid does with a viewer's parcel traffic: how a parcel's record
arrives and what it carries, and what the grid answers when the About Land
floater, the land tool and the objects panel change it. The messages
themselves are described in
[Region & Estate Information](../content/region.md).

Measured on 2026-10-05 on Second Life's beta grid (aditi) and the local OpenSim
standalone by the conformance cases `parcel-properties`, `parcel-crossing`,
`parcel-edit`, `parcel-edit-refused`, `parcel-divide-join`,
`parcel-object-owners`, `parcel-info-dwell`, `parcel-access-list`,
`parcel-ban-enforcement` and `parcel-ban-line`. Each case holds both grids, and
both fake-grid flavours, to these answers as `Measured` constants. The aditi
test accounts own no land and there is no known way for a resident to get some
there (`gridspec-aditi-test-land`), so on Second Life only what a resident
without land rights sees is measured.

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

Read from the raw event (`RUST_LOG=sl_client_tokio::wire=trace` logs each
event-queue event and capability reply before it is decoded). Both grids
send the **same keys** in the same five blocks — `ParcelData` (52 keys),
`MediaData`,
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

## Parcel info

The condensed listing a place profile, a landmark and a search result show for
a parcel **id** (`ParcelInfoRequest` → `ParcelInfoReply`), the id itself
(`RemoteParcelRequest`), and the parcel's dwell (`ParcelDwellRequest`).
Measured by `parcel-info-dwell` on 2026-10-05: on aditi as a resident without
land, in an adult sandbox region whose centre parcel a group owns, and over the
listings of the first rows of a land search and a places search; on OpenSim as
the estate owner, who also put the parcel up for sale and took it off again.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `RemoteParcelRequest` for a location | a parcel id | a parcel id that encodes the region handle and the position (`Util.BuildFakeParcelID`) | the parcel's fixture id, both flavours |
| `ParcelInfoReply` transport | UDP | UDP | UDP |
| a listing for a parcel in another region | answered | answered through the grid's land service (from source) | only the region's own parcels |
| `ParcelDwellReply` | answered, 0 | answered, 0 on a region nobody visits | answered with the fixture's dwell |
| the listing's `Dwell` | 0 for every parcel sampled, busy places included | the dwell module's value | the fixture's dwell |
| `ActualArea` | the parcel's area | the parcel's area | the parcel's area |
| `BillableArea` | the area on some parcels, **0** on others (13 of 16 search rows) | always the area | always the area |
| `SalePrice` of a parcel that is not for sale | still filled: 1, 10000 and 29958 seen on the wire (an `sl-repl` probe) | the stored price | 0 |
| `AuctionID` | 0 on every parcel sampled | the stored id | the parcel's |
| a listing after an edit | not measured (no land) | **stale**: cached for 30 s past its last read, swept every 10 s | follows the edit at once, both flavours |

### The listing's flags byte

`ParcelInfoReply.Data.Flags` is a packing of its own, not the parcel-flags
field of `ParcelProperties` cut down to a byte. The reference viewer reads the
rating and the ownership out of it (`llpanelplaceprofile.cpp`,
`llpanellandmarkinfo.cpp`) and never the sale state.

| bit | meaning | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- | --- |
| `0x01` | region rated moderate | set; **also set for an adult region** | set for moderate only | as its grid |
| `0x02` | region rated adult | set (so adult reads `0x03`) | set (adult reads `0x02`; from source — the local grid has no adult region) | as its grid (`ParcelPolicy::adult_listing_bits`) |
| `0x04` | a group owns the parcel | set (checked against the parcel's own `ParcelProperties`) | set (from source, `LLClientView.SendParcelInfo`) | set when the fixture's owner is a group |
| `0x80` | the parcel is for sale | set on all 8 land-search rows sampled, each at the row's price | set while the estate owner had the parcel for sale, cleared after | set while the parcel carries a sale price |

Every listing's rating was compared with its region's rating on the world map
(`MapNameRequest`) where the map knew the region: on aditi 3 adult (rating bits
`0x03`), 2 moderate (`0x01`) and 2 general (`0x00`). The rows of aditi's places
search name regions its map no longer has, so those were not compared.

`0x04` is also the value the *parcel* flags use for "for sale", which is how
this client read the byte until 2026-10-05: a group-owned parcel was given the
stale price in its `SalePrice` field and a parcel for sale was given none. The
byte is now `ParcelListingFlags`, and `ParcelDetails::sale_price` is `Some`
exactly when `0x80` is set.

OpenSim's cache is keyed by parcel id and each read pushes its expiry out, so
a viewer that polls a listing after an edit never sees the edit. The fake grid
does not imitate it: the wait would cost every offline run a minute and a half
for a lag a viewer cannot act on.

### Land search on Second Life

Aditi answers a `DirLandQuery` over the **event queue**, never over UDP: a
`DirLandReply` event whose body mirrors the UDP message block for block
(`AgentData`, `QueryData`, `QueryReplies`), with a `ProductSKU` string on every
row that the UDP template has no field for (`023` and `024` seen). The client
decodes it into the same `Event::DirLandReply` since 2026-10-05; before that
the event was dropped and a land search on Second Life showed nothing. The SKU
is not carried yet (`protocol-cap-product-info`). OpenSim's standalone answers
a land search with nothing at all, and so does the fake grid
(`server-fake-grid-directory-search`).

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

## Access

Who may enter a parcel: its **allow list** (`AL_ACCESS`) and **ban list**
(`AL_BAN`), read with `ParcelAccessListRequest` and replaced whole with
`ParcelAccessListUpdate`, and what an avatar the lists keep out meets at the
parcel's edge. Measured on 2026-10-05 by `parcel-access-list` (aditi as a
resident without land, on a group's sandbox parcel; OpenSim as the estate owner
and as a resident; both fake flavours), `parcel-ban-enforcement` (OpenSim: the
estate owner bans a second avatar that holds no estate rights) and
`parcel-ban-line` (aditi: a resident walks into somebody else's closed parcel).
The UDP shapes are read from the raw messages: `RUST_LOG=sl_proto::wire=trace`
logs every inbound message, each block and field, before the session reads
anything out of it.

### The lists

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ParcelAccessListReply` transport | UDP | UDP | UDP |
| a request from an agent with no rights over the parcel | answered | answered, with the entries (a banned resident reads the ban on itself) | answered, both flavours |
| the reply's `SequenceID` | 0 | 0 | the request's, which a viewer sends as 0 |
| an empty list | one block: nil id, `Time` 0, `Flags` **0** | one block: nil id, `Time` 0, `Flags` **the list's bit** | as its grid (`ParcelPolicy::empty_list_placeholder_names_its_list`) |
| an entry's `Flags` | not measured (no land) | the list's bit — `0x1` allow, `0x2` ban — whatever the update carried | what the update carried, which from this client is the list's bit |
| an entry's `Time` | not measured | the expiry as written, a Unix time; 0 for never | the same |
| a list longer than one packet | not measured | several replies under the same header: 60 entries came as 48 + 12 | split at 48, both flavours |
| a `ParcelAccessListUpdate` | not measured | not answered: no reply, no alert, no parcel pushed | the same |
| saving a non-empty list | not measured | **switches the parcel's `USE_ACCESS_LIST` / `USE_BAN_LIST` flag on**; emptying the list switches it off | `FakeOpensim` the same; `FakeSl` leaves the flags alone (`ParcelPolicy::list_update_sets_use_flag`) |
| an update from an agent with no rights | refused with a plain `AlertMessage`: "You do not have permission to update the ban list on your group land." | dropped silently | — (the fake grid enforces no land rights) |

A viewer unions the packets of a reply and empties its copy when it *asks*,
not when a packet lands; the reference does the same (`unpackAccessEntries`
inserts into a map `sendParcelAccessListRequest` cleared). In the other
direction the reference cuts a long list into **sections** of 48 entries
(`PARCEL_MAX_ENTRIES_PER_PACKET`), numbered from 1 under one transaction id,
and a simulator replaces the list with the first and appends the rest. This
client sent every entry in one message until 2026-10-05, which at Second
Life's 300-entry limit is a 7 kB datagram; it sends sections now.

OpenSim's flag side effect has teeth. Adding one resident to a parcel's allow
list **closes the parcel to everybody else** without the About Land checkbox
being touched, and nothing tells the viewer: its "anyone can visit" box stays
ticked, and the next About Land save writes that stale flag back and opens the
parcel again (`viewer-about-land-list-save-leaves-flags-stale`).

`USE_ACCESS_GROUP` on its own closes nothing. The flag *admits* the parcel's
group to a parcel that `USE_ACCESS_LIST` has closed; an aditi parcel with only
the group flag let a stranger walk in, and OpenSim reads it the same way
(`LandObject.IsRestrictedFromLand`).

### At the parcel's edge

What an avatar the lists keep out meets. On OpenSim every row is
`parcel-ban-enforcement`; on Second Life `parcel-ban-line`, against a 64 m²
parcel on aditi flagged `USE_ACCESS_LIST` and `USE_ACCESS_GROUP`. A *ban* on
Second Life is not measured: it needs land to ban somebody from.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| who is never kept out | not measured | the parcel's owner, estate owner and managers, administrators (`LandObject.IsBannedFromLand`) | — nobody is kept out (`server-fake-grid-parcel-access-enforcement`) |
| the refusal | `AlertMessage` naming a notification: `NOTIFY: Cannot enter parcel: not a group member` | `AlertMessage` with the text: "You are banned from parcel" / "You do not have access to the parcel" | — |
| walking in | stopped at the line; stays there while it keeps walking | — (flown, below) | — |
| flying in | stopped, then **lifted** straight up — 40 m in two seconds — and let across above the line's top, about 50 m over the ground | crosses the line, is alerted and put back just outside it, again and again while it keeps flying | — |
| teleporting in | not measured | the teleport lands (`TeleportLocal` to the spot asked for); the avatar is alerted and moved out | — |
| banned while standing on the parcel | not measured | nothing until it moves; then alerted and put outside | — |
| after the ban is lifted | not measured | walks in at once | — |
| how high the line reaches | about 50 m above the ground for an allow list (the lifted avatar crossed at 80 m over ground at 35 m; the reference viewer's `PARCEL_HEIGHT` is 50 m); a ban's is not measured — the reference draws 5000 m (`BAN_HEIGHT`) | 100 m above the ground, ban and allow list alike (`BanLineSafeHeight`, from the source, `EnforceBans`) | — |

### The ban line

The fence a viewer draws is not a message of its own. It is an ordinary
`ParcelProperties` for the parcel the avatar may not enter — every block and
field of a normal record — under one of three **sequence ids** the reference
viewer reserves (`COLLISION_*_PARCEL_SEQ_ID`), with `SnapSelection` false and
`RequestResult` 0. Its `Bitmap` is the closed parcel's own squares, which is
where the fence goes. The client reads the three ids as
`ParcelInfo::collision()`.

| sequence id | meaning | Second Life | OpenSim |
| --- | --- | --- | --- |
| `-20000` | not in the parcel's group | sent, for a parcel flagged for both list and group | never sent: OpenSim has only the other two |
| `-30000` | banned | not measured | sent |
| `-40000` | not on the allow list | not measured | sent |

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| transport | event queue | event queue | — |
| when it is pushed | **not pinned down.** Four records 0.4–0.5 s apart, starting 0.2 s after the arrival push, on two of four logins 7–12 m from the parcel; none on a dozen approaches by air and on foot, refused ones included (`gridspec-sl-ban-line-trigger`) | on every significant movement near a parcel that keeps the avatar out, nearest parcel only (`SendOutNearestBanLine`): one or two before the avatar reaches the line, another each time it is put back | — |
| off-switch | — | `ShowParcelBansLines` and `DisableParcelBans` in `[LandManagement]`; never on a tax-free estate | — |

A viewer therefore cannot wait for a ban line to learn that it was refused:
the alert is the refusal, and on Second Life the line may not come at all. The
reference viewer works the same way round — a ban line that arrives is kept,
and it is the "Cannot enter parcel" alert that shows the fence for ten seconds
(`process_alert_message`, `ShowBanLines` in its collision mode).

This viewer raises Second Life's named refusals as the notifications they
name since 2026-10-05; before that it showed the raw
`NOTIFY: Cannot enter parcel: …` string. It does not draw the fence yet
(`viewer-parcel-ban-lines-on-refusal`).
