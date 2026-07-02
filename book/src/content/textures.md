# Textures & the Asset Pipeline

Second Life textures are JPEG-2000 (`.j2c`) codestreams, fetched over the
`GetTexture` [capability](../comms/caps.md). At the lowest level a client issues
`Command::FetchTexture` and receives the raw codestream as
`Event::TextureReceived` — no decoding, no caching, and the caller must manage
level of detail itself.

The **`sl-texture`** crate adds a higher-level pipeline on top of that: it
fetches, **decodes** (via OpenJPEG through the `jpeg2k` crate), and **caches**
textures in memory and on disk, so a texture is never fetched or decoded twice
while it is still referenced. Decoded pixels are canonical 8-bit RGBA, chosen so
they drop straight into a GPU texture.

## Level of detail

A texture's level of detail is a **discard level**: `0` is full resolution and
each higher level halves both dimensions, up to `5`. The `DiscardLevel` newtype
(in `sl-proto`, beside the [`j2c`](../comms/xfer.md) header parser) makes
out-of-range levels unrepresentable and orders so that a *smaller* discard level
is *finer* — "at least as fine as" is `<=`.

A JPEG-2000 codestream is progressive by resolution: a *prefix* of its bytes is
itself a valid, lower-resolution image. So a coarse level only needs the leading
bytes, fetched with an HTTP `Range` request, and OpenJPEG decodes directly to a
target level via its resolution-reduction factor.

## The store

`TextureStore::get(id, target).await` returns an `Arc<TextureEntry>` decoded to
**at least** the requested resolution, fetching and decoding only what is
missing. The store keeps only **weak** references to its entries, so a texture
becomes collectible as soon as the last external `Arc` drops — garbage
collection follows pointer counts, and a periodic `sweep()` prunes dead entries.

Requests for the same texture share work: a per-entry lock makes fetch and
decode **single-flight**, and an in-memory plus on-disk cache means a texture
held in memory (or present on disk) is never re-fetched. Decoding runs on a
`rayon` pool, off the caller's thread, bounded by a CPU-count semaphore. The
codestream and decoded pixels are shared as `bytes::Bytes` / `Arc`, so the data
is not copied as it moves between the fetch, the cache, and consumers.

## Upgrading and downgrading level of detail

One `TextureEntry` represents a texture across *all* levels of detail; its
decoded image is swapped in place, never duplicated per level. `set_lod`:

- **upgrades** to a finer level by fetching more of the codestream and
  re-decoding, and
- **downgrades** to a coarser level by **box-filter downsampling the existing
  RGBA8 pixels** — no re-decode, and independent of whether the codestream is
  still resident. This is how in-memory detail is *lowered* to reclaim memory
  (a `1024²` RGBA image is 4 MiB).

Because a downgrade frees the finer pixel buffer, it must not run while those
pixels are mapped to the GPU. A consumer takes a **lease**
(`entry.lease().await`) while it reads or uploads the pixels; a downgrade takes
the exclusive side of the same lock, so it waits until every lease is released
before swapping in the coarser image and freeing the old one.

## Progress, priority, and cancellation

For a viewer-style workload — many textures requested at once, then the agent
moves — the store offers a scheduling layer. `request(id, target, priority)`
returns a `TextureRequest` handle that is observable, cancellable, and
re-prioritizable:

- **Priority** is an abstract value; how a caller derives it (on-screen size,
  distance, ...) is left to the caller. A texture wanted by several distinct
  on-screen uses is *boosted*: the effective priority is the maximum requester
  priority plus a diminishing popularity term (`floor(log2(count))` scaled), so
  a texture many objects need outranks one few need at the same base priority,
  while urgency stays dominant.
- **Progress** is observable as `TextureProgress`: `Queued`, `ReadingDisk` vs
  `Downloading` (disk cache and network are distinguished), `Decoding`,
  `Ready(level)`, `Failed`, or `Cancelled` — polled or awaited via `changed()`.
- **Cancellation** is interest-counted: dropping the last handle for a texture
  withdraws the request and removes still-queued work, so a moved agent's
  now-irrelevant textures never start. Cancelling one requester never starves
  another that still wants the same texture.

A keyed priority queue admits the highest-priority queued request first when
work is bounded; a request cancelled while queued is removed before it runs.

## The on-disk cache

The store's disk cache is **byte-compatible with the Second Life / Firestorm
viewer's texture cache**, in its own dedicated directory: a `texture.entries`
index, a `texture.cache` file of 600-byte codestream heads, and per-texture
`<hex>/<uuid>.texture` body files. It reads and writes that format, so the cache
survives restarts and mirrors what a viewer would write. Entries are purged
least-recently-used down to a fraction of a size or entry-count budget.

## Using it from the tokio client

`sl-client-tokio` provides `ReqwestTextureFetcher`, a network backend over async
`reqwest` that fetches `GetTexture` byte ranges and whose capability URL the run
loop refreshes on each region change. Build a `TextureStore` over it (with an
optional on-disk cache directory) and drive it with `get` / `request`. The store
surface is re-exported from `sl-client-tokio` (`TextureStore`, `TextureRequest`,
`TextureProgress`, `Priority`, `CacheLimits`, ...).

The `texture-fetch-http` [conformance case](../conformance/overview.md)
exercises this end to end against a live grid: fetch and decode the plywood
texture to RGBA8, confirm a second request is served from cache, and downgrade
then re-upgrade its level of detail.

## Bevy

`sl-client-bevy` integration — a bridge from the store's decoded RGBA8 output to
`bevy::image::Image`, so decoded textures plug into Bevy's rendering — is
forthcoming.

---

The store, decode, cache, and scheduler live in the `sl-texture` crate; the
`DiscardLevel` newtype and `j2c` header parsing live in `sl-proto`; the tokio
network backend and re-exports live in `sl-client-tokio`.

See also [Materials](materials.md), which shade surfaces on top of textures.
