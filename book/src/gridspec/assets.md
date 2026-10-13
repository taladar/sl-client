# Assets

How each grid hands an asset out and how it takes one in: the
asset-delivery capabilities as HTTP first, then [Uploads](#uploads).

What each grid's asset-delivery capabilities — `GetTexture`, `GetMesh`,
`GetMesh2` and `ViewerAsset` — answer as HTTP: the statuses, the ranges, the
headers, and what a request for something that is not there gets. The fetch
itself is described in
[Textures & the Asset Pipeline](../content/textures.md) and
[CAPS](../comms/caps.md).

Measured on 2026-10-12 by the conformance case `asset-http`. The case asks
each capability itself, with a client that follows no redirect and decodes
nothing, about one asset it knows the grid has: the plywood texture, a mesh,
and — over `ViewerAsset` — a body part the avatar wears on OpenSim and a
viewer's UI sound on Second Life (see [What is not there](#what-is-not-there)
for why not the same). It ran four times on aditi and five on the local
OpenSim; every row below is checked on every run of both live grids, and the
four capabilities of a grid answered every question alike unless a row says
otherwise.

An offset is written against the asset's length `N`.

## The capabilities

| question | Second Life | OpenSim |
| --- | --- | --- |
| which are granted | all four | all four |
| where they are | **another host** than the simulator's (`asset-cdn.glb.aditi.lindenlab.com`), over plain `http` | the simulator's own host and port, `http` |
| the URL of each | **the bare root of that host — one URL for all four**, no path, no token, the same for every agent | a path with a token of the capability's own |
| a redirect | never | never |
| `ViewerAsset` asked for a texture (`texture_id`) or a mesh (`mesh_id`) | the asset, under that class's media type | the same |
| the URL without the `/` the reference viewer writes before the `?` | the same answer | the same answer |

## The whole asset

| question | Second Life | OpenSim |
| --- | --- | --- |
| a `GET` with no `Range` | `200`, the asset | the same |
| the media type of a texture, a mesh | `image/x-j2c`, `application/vnd.ll.mesh` | the same |
| the media type over `ViewerAsset` | the class the **request** names: `application/ogg` for `sound_id`, `application/vnd.ll.bodypart`, `application/vnd.ll.clothing` | the class the **asset** is: `application/vnd.ll.bodypart` for a body part whatever the key |
| another `Accept` (`text/html`) | the asset, its media type unchanged | the same |
| `Accept-Encoding: gzip` | the asset, not compressed | the same |
| what a cache is told | `Accept-Ranges: bytes`, an `ETag`, a `Last-Modified`; **no `Cache-Control`**, no `Expires` | **nothing**: no validator, no `Accept-Ranges`, no `Cache-Control` |
| `Content-Encoding` | **`binary/octet-stream`** — a media type where an encoding belongs, on every answer | not stated |
| every header of the answer | `accept-ranges`, `connection`, `content-encoding`, `content-length`, `content-type`, `date`, `etag`, `last-modified`, `x-amz-server-side-encryption` (a texture also `x-amz-meta-s3cmd-attrs`) | `connection`, `content-length`, `content-type`, `date`, `keep-alive`, `server` |
| the `ETag` sent back as `If-None-Match` | **not honoured**: `200`, the asset | no tag to send |
| the `Last-Modified` sent back as `If-Modified-Since` | `304`, no body | no date to send |
| a `HEAD` | `200`, `Content-Length` `N` | the same |
| a `POST` with an empty body | `411`, an HTML error page | **answered as a `GET`**: `200`, the asset |

Second Life's headers are those of an object store behind a proxy (the
`x-amz-*` pair, the `Content-Encoding` that is the object's stored media
type). A client that decodes content encodings has to let that one through.

## Ranges

| `Range` | Second Life | OpenSim |
| --- | --- | --- |
| `bytes=0-9` | `206`, bytes 0–9, `Content-Range: bytes 0-9/N` | the same |
| `bytes=10-19` | `206`, bytes 10–19, `bytes 10-19/N` | the same |
| `bytes=10-` | `206`, bytes 10 to the end, `bytes 10-(N-1)/N` | the same |
| an end past the asset, `bytes=10-(N+99)` | `206`, bytes 10 to the end, `bytes 10-(N-1)/N` | the same |
| the whole asset by range, `bytes=0-(N-1)`, and wider, `bytes=0-(N+99)` | `206`, the asset, `bytes 0-(N-1)/N` | the same |
| **a start at the end, `bytes=N-(N+9)`, past it, `bytes=(N+10)-(N+19)`, and open at the end, `bytes=N-`** | **`200`, the whole asset** | **`200`, the whole asset** |
| the first byte alone, `bytes=0-0` | `206`, one byte, `bytes 0-0/N` | **`200`, the whole asset** |
| a suffix, `bytes=-10` | `206`, the last ten bytes, `bytes (N-10)-(N-1)/N` | **`206`, the first eleven bytes**, `bytes 0-10/N` |
| two ranges, `bytes=0-9,20-29` | `206`, both in a multipart body — under the **asset's own media type**, with no `Content-Range` header | `200`, the whole asset |
| a backwards range, `bytes=19-10` | `200`, the whole asset | `206`, **the one byte at its end**, `bytes 10-10/N` |
| no number, `bytes=x-y`; another unit, `items=0-9` | `200`, the whole asset | the same |

Neither grid ever answers `416`. A range that starts at or past the end of
an asset is answered with the whole asset by both — on OpenSim on purpose
(`GetAssetsHandler`: "viewers do send broken start, then flag good assets as
bad"). So a fetcher that walks an asset range by range must learn where it
ends from a `206`'s `Content-Range`, and must not ask for "the rest" of an
asset it already has: it is sent all of it again.

OpenSim's other differences are its parser's (`Util.TryParseHttpRange`): the
header is split at every `-` and must make two parts; an absent start is
`0`; an end must be above zero; an end before the start is clamped up to it.

## What is not there

| request | Second Life (aditi) | OpenSim |
| --- | --- | --- |
| an id the grid has no asset under, with or without a `Range` | **`503`**, an HTML page "Service Unavailable - DNS failure", no `Retry-After` | `404`, no body |
| the nil id | `GetTexture`: `200`, a 5,854-byte texture; the others `503` | `404` |
| text that is no id, no query, a query key the capability does not know | `503` | `400`, no body |
| an asset of another class — a mesh's id to `GetTexture`, a texture's to `GetMesh`, a sound's id to `ViewerAsset` as `bodypart_id` | `503` | **the asset**, under its own class's media type |
| the body parts and clothes the test avatar wears, over `ViewerAsset` | **none of four answered: `503`** | six of six |

Aditi's `503` is its proxy saying it cannot reach the store behind it, and it
says so for everything it has not already got in hand: a missing asset, a
malformed request, and assets that exist — the avatar's own body parts among
them, on every run. So **aditi cannot tell a missing asset from one its
proxy cannot fetch**, and what Second Life's main grid answers for a missing
asset is not measured here. No answer to an asset the proxy does serve was
ever a `502`, `503` or `504` in four runs.

Second Life keeps an asset under its class: the same id under another
class's key or capability is not found. OpenSim looks an id up whatever the
key says, logs the mismatch and serves it.

## The fake grid

| behaviour | `FakeSl` | `FakeOpensim` |
| --- | --- | --- |
| ranges, the whole asset for a start at or past the end, `HEAD` | as Second Life | as OpenSim, its parser's mistakes included |
| an `ETag`, a `Last-Modified`, `Accept-Ranges`; `304` for the date | stated and honoured (one date for every asset) | not stated |
| a `POST` | `405` | answered as a `GET` |
| a request that names no asset | `404` | `400` |
| a missing asset | **`404`, not aditi's `503`** | `404` |
| the media type over `ViewerAsset` | the class the request names | the class the request names — the store does not keep an asset's class |
| `Content-Encoding: binary/octet-stream` | **not sent** | — |
| where the capabilities are | a host of their own — a second listener on its own port — and all four its bare root | the simulator's, a token each |
| an asset of another class | served: the store does not keep an asset's class | served |

The two `FakeSl` rows in bold are deliberate. A `503` for every missing
asset is a beta grid's proxy failing, not something to build clients
against; and a reference viewer asks its HTTP library to decode content
encodings, so one that honours the header would refuse the answer. The case
holds a `FakeSl` run to everything else and to none of *What is not there*.

The asset host has to be a host of its own: its root *is* the capability,
and the root of the grid's first listener is the login. It answers by
whatever name it is reached, and serves the grid-wide store with no session
— any agent's URL is every agent's.

## The client

- **The asset's length is read from `Content-Range`** (`FetchChunk::total`,
  every fetcher of both runtimes). Before, the texture store asked for the
  bytes up to an upper bound, got the rest of the asset, and — still short of
  the bound — asked for the range starting at the asset's end, which both
  grids answer with the whole asset: **every full-resolution texture was
  downloaded twice**. The mesh store's header probe reads the same total.
- A `416` is still taken for "no more bytes", though neither grid sends one.
- A `502`, `503` or `504` is asked again up to eight times with a doubling
  pause (about 21 s in all) before the fetch fails as unavailable, and the
  viewer then re-issues the texture on a bounded backoff of its own. On aditi
  that is what a missing asset costs.

## Not measured

A missing asset on Second Life's main grid. What a busy asset host answers
(no transient status was seen). The appearance service's baked textures.
Concurrent connections, pipelining and keep-alive limits. The legacy UDP
`RequestImage` and `TransferRequest` roads. OpenSim's `GetTexture` format
conversion and a grid whose assets are on a separate asset server.

## Uploads

What each grid's upload capabilities — `NewFileAgentInventory`,
`MeshUploadFlag`, `InventoryThumbnailUpload` and `SendPostcard` — answer,
what an upload costs and what the grid says of it afterwards. The upload
itself is described in [CAPS](../comms/caps.md).

Measured on 2026-10-13 by the conformance case `upload-http`. An upload is
two requests: LLSD metadata to the capability, answered with an `uploader`
URL, and the asset's bytes to that URL, answered with the new asset and the
new item. The case sends both itself and asks one question a request; after
each upload it listens for what the grid sends unasked and then asks for the
balance. It ran five times on aditi and seven on the local OpenSim, and every
row below is checked on every run of both live grids unless it says
otherwise. A run on aditi spends about L$ 100.

### The price

| question | Second Life | OpenSim |
| --- | --- | --- |
| where a viewer reads the price | the login's benefits package: L$ 10 a texture, a sound or an animation, L$ 50 a texture above 1024×1024 | `EconomyData`'s `price_upload` — **which OpenSim does not charge** (below) |
| the first answer | `{state: "upload", uploader, upload_price, balance}` — the class's price and the agent's balance | `{state: "upload", uploader, upload_price: 0}` |
| `expected_upload_cost` in the first request: right, absent, `0`, `1`, one too many, a thousand million, negative | **not looked at**: an uploader for each | not looked at |
| `expected_upload_cost` as text | refused: `Upload_BadParameterType` | an uploader |
| the bytes, where the request stated **no** cost | taken, **charged the price** | taken, free |
| the bytes, where it stated the right cost | taken, charged | taken, free |
| the bytes, where it stated another cost — `0`, `1`, one too many, negative | refused: `Upload_UploadPriceDiffers`, *The server expects a different upload fee*, with `expected_upload_price` and `upload_price`; nothing charged | taken, free |
| the bytes, where it stated more than the balance | refused: `Upload_InsufficientLindenDollarBalance`, with the balance and the stated cost as `upload_price` | taken, free |
| a 2048×2048 texture at the small texture's price, a 64×64 one at the large one's | refused: `Upload_UploadPriceDiffers` (`upload_price` 50, and 10) | taken, free |
| an agent with L$ 3, at the first request | refused: `Upload_InsufficientLindenDollarBalance`, with `balance` and `upload_price` | an uploader, and the upload taken: every upload is covered |
| what a charge is followed by | a `MoneyBalanceReply` with no transaction block, within four seconds | nothing |
| what a refusal is followed by | nothing | — |

Second Life holds the stated cost to the **bytes**, not to the request: only
the bytes say whether a texture is a large one. So a viewer has to state the
price of what it is about to send, or state none. OpenSim's upload charge is
its money module's (`IMoneyModule.UploadCharge`, `0` in the stock module),
not the `PriceUpload` its `EconomyData` quotes: the local grid quotes L$ 7
and charges nothing.

### The classes

| class (`asset_type` / `inventory_type`) | Second Life | OpenSim |
| --- | --- | --- |
| `texture` / `texture`, `texture` / `snapshot` | taken, L$ 10 (50 large) | taken; an item of class texture, and of inventory class snapshot for the second |
| `sound` / `sound`, `animatn` / `animation` | taken, L$ 10 | taken, as a sound and an animation |
| `bodypart` / `wearable`, `clothing` / `wearable` | **taken, L$ 10** — an uploader at the first request, `Upload_UploadPriceDiffers` for a stated cost of `0` | taken, as a body part and as clothing, flags `0` |
| `notecard`, `lsltext`, `gesture`, `settings`, `material` | refused at the first request: `Upload_InvalidAssetType`, *Invalid asset type: …* | taken, **and filed as a texture**: asset class `0`, inventory class `0` |
| `callcard`, `landmark`, `object` / `object`, a class nobody has | refused at the first request with **`500`**: `Upload_ServerError`, *Unable to upload asset.* | an uploader; `landmark` filed as a texture |
| `object` / `object`, its bytes | — | answered `complete` with a new asset and a new item, **neither of which exists** — and every later first request of the session is answered *Uploader busy processing previous request* |
| `texture` / `sound` | refused: `Upload_InvalidInventoryType`, with `valid_inventory_types` | an uploader |
| `mesh` / `mesh`, or a model with no `asset_resources` | refused: `Upload_InvalidAsset`, `INVALID_MESH_CONTENTS`, naming the missing lists | refused: *missing model information.* |

OpenSim's completion handler knows five inventory classes by name (`sound`,
`snapshot`, `animation`, `wearable`, `object`) and leaves every other at its
starting value, a texture's. An object that is no mesh model makes it return
without resetting its state (`BunchOfCaps.UploadCompleteHandler`), which is
what leaves the uploader busy until the agent logs in again.

### The first request

| request | Second Life | OpenSim |
| --- | --- | --- |
| no `folder_id`, no `asset_type`, no `inventory_type`, no `name` | refused: `Upload_MissingParameter`, *Missing parameter '…'* | an uploader |
| an empty map | refused: `folder_id` missing | an uploader |
| the nil folder, a folder nobody has | an uploader; the upload is taken, charged, and **makes an item** | the same, free |
| no body | refused: `Upload_BadParameterType` (`'body' is <type 'bool'>`) | `500`, no body |
| a body that is no LLSD | refused: `Upload_InvalidRequestBody` | `500`, no body |
| a `GET` | `501` | `404`, an HTML page |

A Second Life refusal is `200` with `{state: "failure", message, error:
{identifier, message, …}}` (the classes it answers `500` aside); an OpenSim
one is `200` with `{state: "error", error: {identifier: nil, message},
upload_price: 0, uploader: ""}`.

### The bytes

| bytes | Second Life | OpenSim |
| --- | --- | --- |
| a texture, a sound, an animation | `{state: "complete", new_asset, new_inventory_item, new_next_owner_mask, new_group_mask, new_everyone_mask, new_base_mask, new_owner_mask, inventory_flags, upload_price, transaction_id, balance}` — both full masks `0x7fffffff`, `balance` undefined | `{state: "complete", new_asset, new_inventory_item, new_next_owner_mask, new_group_mask, new_everyone_mask, inventory_item_flags}` — `new_asset` as text |
| nothing, as a texture | refused: `Upload_InvalidAsset`, *File is empty* | taken |
| noise, a PNG, as a texture | refused: `LLKDUMessageError::flush()` | taken |
| a texture 100×60 | refused: *Invalid width: Value not a power of 2.* | taken |
| nothing, noise, a WAV file, as a sound | refused: *Internal error from verifier.* | taken |
| a sound in two channels, a sound at 22 050 Hz | **taken**, charged | taken |
| a sound of 31 s | refused: *Sample too long.* | taken |
| nothing, noise, half an animation | refused: *Bad .anim file …* | taken |
| an animation that states 61 s | **taken**, charged | taken |
| the same uploader a second time | `404`, the words *cap not found* | `200`, no body |
| a `GET` to an uploader | `501` — **and the uploader is spent**: the bytes posted after it get the `404` | `404`, an HTML page; the bytes posted after it are taken |
| where the uploader is | on the capability's host | on the capability's host |

A refusal of the bytes costs nothing. OpenSim validates nothing: what is
posted is what is stored.

### A mesh

| question | Second Life | OpenSim |
| --- | --- | --- |
| the first answer for a unit cube | `upload_price` **11**, and `data`: `upload_price_breakdown` (`model` 10, `mesh_instance` 1, `mesh_streaming`, `mesh_physics`, `texture` 0), `resource_cost` 0.5, `model_streaming_cost` 0.05, `simulation_cost` 0.5, `physics_cost` 0.36, `streaming_cost`, `streaming_params` (bytes per level), `model_physics_cost`, the counts of meshes, instances, textures and rigged meshes | `upload_price` 0, and `data`: `upload_price_breakdown` all zero, `resource_cost` 0.5, `model_streaming_cost` 0.064, `simulation_cost` 0.5, `physics_cost` 0.36 |
| a model with no `physics_convex` block | refused: `MAV_BLOCK_MISSING`, naming the block and the mesh | refused: *Missing physics_convex block* |
| a mesh that is noise | refused: `INVALID_MESH_CONTENTS`, *can't parse header* | refused: *Invalid model data* |
| no meshes, no instances | refused: *The model contains no meshes.* / *no instances.* | refused: *missing model information.* |
| an agent that cannot pay | refused at the first request, `upload_price` 11 | the cost data: every upload is covered |
| the upload | **not measured**: refused, `Upload_MeshUploadStatus` — see the flag below | taken: an object in inventory (asset class 6), free |

### `MeshUploadFlag`

| question | Second Life | OpenSim |
| --- | --- | --- |
| a `GET` | the agent's name record (`id`, `username`, `display_name`, the legacy names, …) and `mesh_upload_status` | `{mesh_upload_status: "valid"}` |
| a `POST` | the name record without the status | `404` |
| the status | **`invalid`** for both test accounts asked — the account's own, set once it has done what Linden Lab asks before a first mesh upload | `valid`, always |

The status is the account's, so it is recorded and not held. With it
`invalid` the mesh upload itself is refused (*Mesh upload status is not
valid.*) after the first answer has quoted a price.

### `InventoryThumbnailUpload`

Second Life only. Free; nothing is sent after it.

| question | Second Life |
| --- | --- |
| the first request: an item, a folder, an item in a prim nobody has, an item nobody has, the nil item, both ids, no id at all | **an uploader for every one**: `{state: "upload", uploader}` |
| a 256×256 image for an item, for a folder | `{state: "complete", new_asset}`; the item then states `thumbnail: {asset_id}` over `InventoryAPIv3` |
| a 32×32 image, a 256×128 one | taken |
| a 512×512 image | refused: `{state: "failure", message: "Invalid width: Value too large."}` |
| noise, nothing | refused, in the texture uploader's words |
| an image for an item nobody has, for no id | **`500`** |
| an image for an item in a prim nobody has | taken |
| a `GET` | `501` |

A refusal here is `{state, message}` with no `error` beside it.

### `SendPostcard`

Second Life only. The first request — a card to an address in a reserved
domain, a card with no address, one whose address is no address, an empty
map — is answered with an uploader every time, and a `GET` with `501`. **The
second request is not measured**: it sends a real e-mail, and the case sends
one only when the fixtures name an address (`postcard_to`).

### The fake grid's uploads

| behaviour | `FakeSl` | `FakeOpensim` |
| --- | --- | --- |
| the classes taken and refused, each refusal's status, identifier and words | as Second Life | as OpenSim: everything taken, unknown classes filed as textures |
| the stated cost held to the bytes; the prices | as Second Life, at L$ 10 / 50 | not looked at, free |
| the bytes validated | a texture's size and shape, a sound's container and length, an animation's layout — **the words for an unreadable animation are its own** | not at all |
| a balance | stated in the first answer and charged, L$ 10 000 a session; an upload it does not cover refused. **Not a ledger**: nothing else reads it, no balance request is answered and no balance is pushed after a charge | none |
| an uploader | serves once; spent by a `GET` | serves once; a second post `200`, no body |
| an object that is no model | refused (`500`) | answered `complete`, nothing stored, the uploader busy from then on |
| a mesh's first answer | the price alone: **no cost data**, and the meshes are not read | `upload_price` 0, no cost data |
| `MeshUploadFlag` | the name record and `valid` | `valid` |
| `InventoryThumbnailUpload`, `SendPostcard` | granted, as measured; the thumbnail is stored but **not set on the item** — the inventory has no place for one yet; a postcard is handed to the driver, not mailed, and answered `complete` | not granted |

The rows in bold are what a fake run is not held to: what an upload was
charged, somebody who cannot pay, a mesh's cost data and bytes, and a
thumbnail read back, on both flavours; the balance pushed after a charge and
the two animation refusals' words on `FakeSl`.

### The client and uploads

- `expected_upload_cost(class, benefits, economy)` is the price a viewer
  states: the benefits package's where the login carried one, the
  `EconomyData` upload price otherwise.
- A refusal's words are read from `error` (a text, or a map's `message`) and,
  since this was measured, from a bare `message`: without that a refused
  thumbnail failed as "upload did not complete (state failure)".
- `Command::RequestMeshUploadFlag` → `Event::MeshUploadFlag`;
  `Command::UploadInventoryThumbnail` → `Event::InventoryThumbnailUploaded`
  or `…Failed`; `Command::SendPostcardViaCaps` → `Event::PostcardSent` or
  `Event::PostcardFailed`. Each fails at once where the region grants no
  such capability, the flag aside: there nothing is answered, and the
  reference viewer takes that for leave to upload.
- **The viewer makes new wearables and new materials over
  `NewFileAgentInventory`**, stating a cost of `0`. OpenSim takes both (and
  files the material as a texture); Second Life refuses the material by name
  and the wearable for its price. The reference viewer creates the item
  first and saves into it. Both are filed as bugs
  (`viewer-new-wearable-refused-on-second-life`,
  `viewer-new-material-refused-on-second-life`).

### Not measured about uploads

A mesh upload on Second Life, and the main grid's prices for a premium
account. A postcard's second request. A thumbnail for a folder nobody has,
and for an item in a prim that exists. An upload into a folder of another
agent's. A texture above 2048 pixels, a sound in another codec inside an Ogg
container, an animation with constraints. What OpenSim charges under a money
module that charges. The legacy UDP `AssetUploadRequest`, which wearables
are saved by.
