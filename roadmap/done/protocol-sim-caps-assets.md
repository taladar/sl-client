---
id: protocol-sim-caps-assets
title: Server-side asset delivery caps — GetTexture, GetMesh, ViewerAsset
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 8
blocked_by: [protocol-sim-caps-framework]
---

Context: [context/protocol.md](../context/protocol.md).

The server side of `GetTexture`, `GetMesh`, `GetMesh2` and `ViewerAsset`:

- HTTP Range request parsing and 206 partial-content responses (the
  client fetches mesh LODs and progressive JPEG2000 by byte range);
- correct content types per asset kind;
- an asset-store fixture trait (in-memory + directory-backed) so the fake
  grid and loopback tests can serve real texture/mesh bytes.

Mirrors the client fetch paths in `sl-client-tokio/src/assets.rs` /
`sl-client-bevy/src/assets.rs`; verified by round-tripping against them
in-memory.

Done (2026-08-14): the four caps landed on a **session-free** surface,
`AssetCaps` (`sl-proto/src/asset_caps.rs`), rather than on `SimCaps` —
because on Second Life these are served by a CDN on a different host from
the simulator (and avatar baking is a third, separate service). It
dispatches against an `AssetSource` byte store (`asset_source.rs`:
UUID-keyed trait + pure `InMemoryAssetSource`; the directory-backed
fixture is the eager `load_asset_dir` loader in `sl-client-tokio`, keeping
`sl-proto` sans-I/O). `CapsRequest` grew a `range` field and
`CapsResponse` a `content_range` field for the byte-range path: no
`Range` → `200` whole; satisfiable range → `206` +
`Content-Range: bytes s-l/total`; overrun on an existing asset → `416`
(spec-correct, diverging from OpenSim's whole-asset quirk — our client
maps `416` → empty chunk); miss → `404`; non-GET → `405`. `ViewerAsset`
classifies the request by the new `AssetType::from_asset_query_key`
inverse. `SimCaps` composes one `AssetCaps` so a single seed grant
advertises every cap (`SimCaps::new` co-located, `new_split` for a
distinct CDN base, `from_tokens` to rebuild the surface in a CDN
process); the coverage table's four rows flipped to Served and its
predicate now consults both registries. Loopback tests
(`sl-proto/tests/sim_caps.rs`) drive the 200/206-loop/416/404 contract;
`sl-client-tokio/tests/asset_caps_roundtrip.rs` drives the **real**
`sl_asset::AssetStore` through a shim fetcher against `AssetCaps::dispatch`.
Book coverage: the "asset-delivery handlers" subsection of
`book/src/comms/caps.md`.
