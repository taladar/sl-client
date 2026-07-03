# sl-asset

A higher-level generic-asset fetch API and Firestorm-compatible on-disk cache
for Second Life / OpenSim clients — the opaque-asset counterpart of
[`sl-texture`](../sl-texture) and [`sl-mesh`](../sl-mesh), sharing their
scheduling primitives via [`sl-asset-sched`](../sl-asset-sched).

Textures and meshes each have a dedicated capability (`GetTexture`,
`GetMesh2` / `GetMesh`) and a decode step (JPEG-2000 → RGBA, LLMesh →
geometry) with level-of-detail management. Every *other* asset class — sounds,
animations, landmarks, notecards, gestures, body parts, clothing, and so on —
is an opaque blob fetched whole over the single `ViewerAsset` capability, the
modern replacement for the legacy UDP `TransferRequest` path. Both Second Life
and OpenSim expose `ViewerAsset`.

`AssetStore` therefore does only caching and de-duplication, not decoding:

- **Weak-reference sharing.** The store hands out `Arc<AssetEntry>` and keeps
  only `Weak` references, so an asset is collected once the last consumer drops
  it (pointer-count GC). A repeat `get` for a still-referenced asset returns the
  same shared `Arc` with no re-fetch.
- **Single-flight.** Concurrent `get`s for the same asset share one download.
- **On-disk cache.** Fetched bytes are written to a dedicated directory as
  `<hex>/<uuid>.asset` (sharded by the id's first character, as the viewer
  shards its disk cache), LRU-purged against byte and entry ceilings, and served
  on later runs without a network round-trip.
- **Bounded concurrency.** Fetches are admitted through the shared
  `sl-asset-sched` priority gate.

The network transport lives behind the `BlobFetcher` trait (an
`AssetFetcher<AssetRef>`), so the same store core runs under either the tokio
client (async `reqwest`) or the Bevy client (blocking `reqwest` on a task pool).
An `AssetRef` is an `(id, AssetType)` pair, because the `ViewerAsset` fetch URL
selects the asset by a class-specific query parameter (`?sound_id=`,
`?bodypart_id=`, …).

## License

Licensed under the GNU Lesser General Public License, version 2.1 only
(LGPL-2.1-only), matching the Second Life reference viewer.
