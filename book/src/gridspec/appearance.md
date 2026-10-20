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

### Not measured

- A bake while another viewer is in a neighbouring region, and an appearance
  sent down a child circuit (see [teleport](teleport.md) for what a child
  circuit carries).
- The viewer's own bakes uploaded over `UploadBakedTexture` on OpenSim, which
  the `baked-texture-upload` case exercises.
- How Second Life computes a bake's id.
- An OpenSim region with XBakes, where a cache hit is expected to survive the
  login.
