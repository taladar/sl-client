# Assets

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
