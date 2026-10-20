# Appearance

How each grid makes an avatar look the way it does: who composites the worn
layers into the baked textures, what the agent and the viewers around it are
told, and what each of the protocol's two baking roads answers on each grid.
The protocol side is in the *Content Layer*'s
[appearance chapter](../content/appearance.md).

## Baking

Second Life **central-bakes**: the grid composites the Current Outfit Folder
into the baked slots, and every viewer fetches them by URL from the grid's
appearance service. A stock OpenSim region leaves it to each viewer, which
composites its own avatar, uploads the bakes (`UploadBakedTexture`),
publishes them (`AgentSetAppearance`) and asks the region's cache for bakes
it published before (`AgentCachedTexture`).

Measured on 2026-10-19 and 2026-10-20 by the conformance case
`appearance-baking`: avatar A asks, avatar B stands beside it and only listens.
Twelve runs on aditi (Mauve, the build location) and nine on the local OpenSim
(Northeast Region), the last of each with every answer held. COF versions grow
from run to run, so the case states each version by what it is — the one asked
for, the one the grid last stated (*current*) — never as a number.

### Who bakes

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the login's `agent_appearance_service` (**held**) | named, an `http` URL ending in `/` | absent | as the flavour's grid |
| the login's `cof_version` (**held**) | named | absent | the same |
| `RegionProtocols` bit 0, central baking (**held**) | set | clear | the same |
| the seed grants `UpdateAvatarAppearance` and `IncrementCOFVersion` (**held**) | both | neither | the same |
| the seed grants `UploadBakedTexture` (**held**) | no | yes | the same |

### What an avatar is told on arrival

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the agent's own `AvatarAppearance` (**held**: at least one) | one | one | one |
| its `AppearanceData` block (**held**) | version 1, the COF version the login named, flags 0 | no block | as the flavour's grid |
| an `AppearanceHover` block (**held**) | yes | yes | yes |
| the baked slots | all eleven baked but the skirt, which holds the default placeholder (the test avatars wear no skirt) | every slot the default placeholder: nothing has ever baked these avatars on the local grid | `FakeSl`: the outfit's bakes, the slots nothing feeds the default; `FakeOpensim`: the fixture's solid bakes |
| B's of A (**held** live) | one, with the same bakes as A's own | one or two, the same bakes | — (B is shown nobody) |

### The appearance service

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| A's head bake, fetched as the reference fetches it (`<service>texture/<agent>/head/<id>`) (**held**) | `200 image/x-j2c`, a J2C codestream | no service | as the flavour's grid |
| a texture nobody baked (**held**) | `404 application/llsd+xml`: `{agent_id, code: 2, debug_cof: undef, error: "Hash mismatch: <id>", success: false}` | no service | the same |

### The cache

`AgentCachedTexture` asked about the eleven baked slots, with nil cache ids,
then with made-up ones, and after the publish below with the ids it named.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| any query (**held**) | **never answered** | eleven entries in the order asked, the serial echoed | as the flavour's grid |
| nil or made-up cache ids (**held**) | — | no hits | the same |
| the ids a publish named (**held**) | — | a hit on every slot: the published texture | the same |
| a `RebakeAvatarTextures` (**held**) | never sent | never sent | never sent |

OpenSim answers from the avatar's `WearableCacheItems`, which only an
`AgentSetAppearance` fills; the reference only asks where the region does not
central-bake (`LLAgentWearables::queryWearableCache`).

### The bake handshake

Raw `UpdateAvatarAppearance` POSTs of `{cof_version}`, then our client's own
request.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| version 0 (**held**) | `400`: `success` false, `code` 1, `error` "Cof Version Mismatch: asked != current", `expected` the current version, `observed` and `cof_version` the one asked; nothing sent to either viewer | no capability | as the flavour's grid |
| the version expected (**held**) | `200`: `success` true, `error` undef, `cof_version` the one asked; an `AvatarAppearance` at that version to A and to B | no capability | the same, to A |
| the same version again (**held**) | accepted and stated again, exactly as the first time | no capability | the same |
| five ahead of it (**held**) | refused as version 0 is | no capability | the same |
| our client's request at 0 (**held**) | read as `success` false with the expected version | no capability | the same |
| `IncrementCOFVersion`, then a bake at its answer (**held**) | one above the current version; the bake accepted | no capability | the same |
| the keys of an answer (**held**) | refused: `agent_id code cof_raw cof_version debug_cof error expected observed success`; accepted: `agent_id avatar_scale cof_version error success textures visual_params warnings` | — | the same |
| an accepted bake's payload (**held**) | `textures`: the 45 face ids of the `AvatarAppearance` that follows, 34 of them nil; `visual_params`: its 253 bytes, as integers; `avatar_scale`: `[0.45, 0.6, height]` | — | the same, baked from the outfit |
| a refusal's payload (**held**, as an outline without counts) | `cof_raw`: the Current Outfit Folder's links and the items they resolve to, a wearable's with its asset text (`contents`) and `wearable_type`; `debug_cof`: the same scale and parameters, each slot's `hash_id` and layer `texture_ids`, the worn items in seventeen lists by wearable type, the rigged meshes' joint offsets, the attachments' render cost (`lod`) | — | the same; no attachments, so no joint offsets and a zero `lod` |
| a slot's `hash_id` (**held**) | the id of the bake on that slot when some layer feeds it; the default placeholder when none does (the skirt of an avatar wearing none) | — | the same |
| what a `hash_id` is | **not** the reference viewer's wearables hash (`LLWearableData::computeBakedTextureHash`) on any slot, nor an MD5 of the slot's layer ids; the same within a run | — | the grid's own MD5 of the slot, its layers and the parameters |

The payload was read by the case's second pass (2026-10-20, four aditi runs):
the bake an accepted answer describes is the appearance that follows,
exactly. Second Life names each bake by an id it states as the slot's
`hash_id`; how it computes it is unknown, so the fake grid names its bakes by
its own hash of what each is made of, and holds the relation rather than the
id.

In one aditi run of nine Second Life sent the agent its appearance **twice**
after an accepted bake; the case holds that the agent was told, not how often.

The first runs read the same-version bake as "no answer": the request had
gone out on a kept-alive connection the grid had already closed. The case
opens a fresh connection per request since.

### The legacy publish

An `AgentSetAppearance` of the plywood texture on every baked slot, neutral
visual parameters and made-up cache ids; then A's arrival appearance
published back.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| what A is told (**held**) | nothing | **nothing**: OpenSim sends the appearance to every other viewer only | nothing |
| what B is told (**held** live) | nothing | the publish, verbatim: no block, the plywood on every slot | — |
| it is kept (**held**) | no | yes, for the session (see [Across a login](#across-a-login)); an earlier run's publish was seen again at the next run's login, which is why the case publishes A's arrival appearance back | kept for the cache on `FakeOpensim`, per session |
| the arrival appearance published back (**held** live) | — | B is sent the arrival bakes again | — |

Second Life's central-bake regions ignore the message; the reference never
sends it there (`LLAgent::sendAgentSetAppearance` returns on a region with a
central-bake version).

### Across a login

A logs in again straight after the publish, and asks the cache once more
under the ids the publish named.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the new login's `cof_version` (**held**) | the version the grid last stated: the folder's, not the session's | absent | the same; `FakeSl` keeps each account's version across logins |
| A's own arrival appearance (**held**: told, block, hover) | as at the first arrival | no block, the **default placeholder** on every slot, not the publish | as the flavour's grid, but `FakeOpensim` names its fixture bakes |
| B's of A (**held** live) | as A's own | the same as A's own | — |
| the published ids asked again (**held**) | never answered | eleven entries, **no hits** | the same |

OpenSim's cache is the avatar's `WearableCacheItems` in the region's memory;
at a login it keeps only a slot whose texture its asset cache still holds, or
asks the optional XBakes service (`[XBakes] URL`, unset in a stock
`OpenSim.ini` and on the local grid) for the bakes it stored
(`AvatarFactoryModule.ValidateBakedTextureCache`). A stock region therefore
answers a query with a hit only within the session that published: after an
outfit change, for each slot the change left alone. The reference asks at
login and at every outfit change (`LLAgentWearables::queryWearableCache`).

### Not measured (baking)

- A bake while another viewer is in a neighbouring region, and an appearance
  sent down a child circuit (see [teleport](teleport.md) for what a child
  circuit carries).
- The viewer's own bakes uploaded over `UploadBakedTexture` on OpenSim, which
  the `baked-texture-upload` case exercises.
- How Second Life computes a bake's id.
- An OpenSim region with XBakes, where a cache hit is expected to survive the
  login.

## Attachments

What each grid answers every way a viewer wears and takes off an object, and
what the agent and a second viewer beside it are sent. Measured on 2026-10-20
by the conformance case `attachments`: avatar A wears copies of a cube, each
copy known by a role (`cube1`, `cube2`, `hud`, …), and avatar B stands beside
it and only listens. Three runs on aditi (Mauve, the build location) and four
on the local OpenSim (Northeast Region), the last of each with every answer
held. The census wears on the first two body points and the first HUD point
nothing of A's is on (the aditi avatars wear an animation overrider and six
other attachments), so a point is named by which of those it is, never by
number.

### What an attachment is

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a worn item's object (**held**, as A and B see it) | a new object, parented to the avatar, on the point asked, with the same local id for both viewers | the same | not modelled: the fake grid drops every attach message |
| its name-values (**held**) | `AttachItemID` only, naming the worn item, to both viewers | the same | — |
| its `OwnerID` (**held**) | **nil**, to both viewers | nil | — |
| its update flags (**held**) | A: `0x1002093c`; B: `0x10020810` — B is told the owner may modify it | A: the same; B: `0x00020810` | — |
| a HUD's object (**held**) | sent to A only; B is never sent it | the same | — |
| a HUD taken off (**held**) | B is sent a `KillObject` for the object it never saw | no kill to B | — |
| a script's temporary attachment (`llAttachToAvatarTemp`) (**held**) | `AttachItemID` is the **object's own id**; no item is made | the same | — |
| the agent's `AvatarAppearance` after a wear (**held**) | sent again, its attachment list naming what is worn | not sent | — |

The temporary attachment's own id is what the reference's
`LLViewerObject::isTempAttachment` tests, and what `Object::is_temp_attachment`
tests here.

### Wearing from inventory

`RezSingleAttachmentFromInv` of one item at a time.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| a free point, *replace* (**held**) | worn; no inventory message | the same | — |
| a second item *added* to it (**held**) | both worn | the same | — |
| a third *replacing* them (**held**) | **one** of the two (the first worn) is killed and its item restated; the other stays | **both** are killed and restated | — |
| an item already worn, again on another point (**held**) | ignored: it stays where it was | the same | — |
| an item nobody has (**held**) | an alert, "Attempt to rez an object failed." (`RezAttemptFailed`) | nothing | — |
| several at once (`RezMultipleAttachmentsFromInv`, added) (**held**) | all worn | the same | — |

OpenSim's *replace* takes off everything on the point under `[Attachments]
WearReplacesAll`, true by default (`AttachmentsModule.AttachObjectInternal`);
set to false, it takes off the first, as Second Life does.

The reference packs four objects to a `RezMultipleAttachmentsFromInv` packet,
every packet naming one compound message and the total; our client sent them
all in one, which an outfit of a few dozen attachments would not fit in a
datagram. It packs four too since 2026-10-20.

### Taking off

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ObjectDetach` by the object (**held**) | killed for both viewers; the item restated, on the event queue (`BulkUpdateInventory`) | killed; the item restated over UDP (`UpdateCreateInventoryItem`) | — |
| `DetachAttachmentIntoInv` by the item (**held**) | killed; **no** inventory message | the same | — |
| two in one `ObjectDetach` (**held**) | both killed; one item restated | the same, over UDP | — |
| a drop (`ObjectDrop`) (**held**) | sent again **unattached under the same ids**, no kill; the item **removed** from the inventory | the same | — |
| a dropped object (**held**) | no name-values; `OwnerID` nil | the same | — |

### From the world, and by a script

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `ObjectAttach` of a rezzed cube (**held**) | sent again attached under the **same ids**; a new item announced over UDP, which its `AttachItemID` names | the same | — |
| taken off again (**held**) | killed; the item restated on the event queue | killed; the item restated over UDP | — |
| a probe asking for `PERMISSION_ATTACH` (**held**) | a `ScriptQuestion` for 32; granted, `run_time_permissions(32)` | the same | — |
| `llAttachToAvatar` (**held**) | as `ObjectAttach`: same ids, an item made; the script's `attach` event names A and `llGetAttached` the point | the same | — |
| `llAttachToAvatarTemp` (**held**) | same ids, **no** item; `AttachItemID` its own id | the same | — |
| `llDetachFromAvatar` of a temporary attachment (**held**) | killed and gone: not sent back into the world, no item | the same | — |
| `llDetachFromAvatar` of the other (**held**) | killed; the item restated on the event queue | the same, over UDP | — |

### The limit

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the login's `attachment_limit` (**held**) | 38 (the `Base` package) | no benefits | — |
| `SimulatorFeatures` `MaxAgentAttachments` (**held**) | 38 | 38 (`Constants.MaxAgentAttachments`) | — |
| one more than the limit, in one compound message (**held**) | 38 worn, the last one refused **without a word** | the same | — |

### Across a teleport and a login

A teleport to another region (a neighbour on OpenSim; on aditi, where Mauve has
none on the map, the first of the fixtures' `ban_line_regions` the map knows —
it answers a name it does not know, such as `Plum`, with one placeholder block
at grid 0,0 under access 255) and
back, with `cube1` worn, B staying behind; then a new login with it worn.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the attachment in the destination (**held**) | sent anew, the **same full id**, a **new local id**, parented to the avatar's new local id | the same | — |
| the region left, before the arrival (**held**) | kills A's avatar for A, never the attachment | kills nothing | — |
| B, left behind (**held**) | sent a kill of A's avatar, **none** of its attachments | sent nothing | — |
| back in the region left (**held**) | a new local id again; B is not sent A (the telehub is out of its sight) | the local id it had in the region it came from, kept; B is sent it again | — |
| a new login with an item worn (**held**) | the grid puts it back on unasked, with everything else A wore | the same | — |
| the agent's `AvatarAppearance` at the arrival (**held**) | lists it | sent, listing nothing | — |

So a viewer keys an attachment by region and local id, and drops an avatar's
attachments with the avatar: a Second Life region never kills them on its own.
Neither grid needs the viewer to re-wear the Current Outfit Folder's
attachments at a login.

### Not measured (attachments)

- Attachments of a linkset, and an attachment's position and rotation offsets.
- Animated-object (animesh) attachments and their own limit.
- A HUD's scripts and touches; another avatar's view of a scripted
  attachment's chat.
- A wear while the agent is still arriving, and two wears of one item racing.
- What the fake grid answers: it models none of it
  (`fake-grid-own-attachments-and-region-moves`).
