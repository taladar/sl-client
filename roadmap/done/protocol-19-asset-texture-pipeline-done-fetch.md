---
id: protocol-19
title: Asset & texture pipeline (done, fetch)
topic: protocol
status: done
origin: ROADMAP.md — Tier C
---

Context: [context/protocol.md](../context/protocol.md).

**19. Asset & texture pipeline (done, fetch) ✅ — CAPS
`GetTexture`/`GetMesh`/`GetMesh2`/`GetAsset`, legacy `RequestImage` +
`ImageData`/`ImagePacket`/`ImageNotInDatabase` and
`TransferRequest`/`TransferInfo`/`TransferPacket` · 13 pts.** Fetches a texture,
mesh, or generic asset by UUID over **both** transports — usable alone as an
asset fetcher given known UUIDs, and the substrate for textured rendering (#20),
animations (#21) and sound (#22). Per the scope decision, this delivers the
**fetch** layer only; actual JPEG-2000 pixel decode is out of scope (the bytes
are surfaced raw). Implemented:

- **Legacy UDP textures** — `Session::request_texture(id, discard_level,
  priority)` (`RequestImage`), with `ImageData` (the codec/size/packet-count
  header plus packet 0) and `ImagePacket` (follow-ups) reassembled by packet
  index into a `Texture { id, codec, data }` (`Event::TextureReceived`);
  `ImageNotInDatabase` → `Event::TextureNotFound`. `discard_level` is the native
  LOD knob (the sim streams from that level up).
- **Legacy UDP generic assets** — `Session::request_asset(id, AssetType,
  priority)` (`TransferRequest` on channel/source `LLTST_ASSET` = 2, params =
  UUID ++ little-endian `LLAssetType`), with `TransferInfo` (size/status) then
  `TransferPacket` chunks reassembled in order until `LLTS_DONE` → `Asset { id,
  asset_type, data }` (`Event::AssetReceived`); a non-success status →
  `Event::AssetTransferFailed`.
- **HTTP CAPS** — runtime commands `FetchTexture { texture_id, discard_level }`
  (`GetTexture`, `?texture_id=`), `FetchMesh` (`GetMesh2`/`GetMesh`,
  `?mesh_id=`) and `FetchAsset { asset_id, asset_type }` (`ViewerAsset`, by
  class),
  HTTP-GET on a background task and surfaced as the same `Texture`/`Asset`
  events. The seed now also requests these four caps.
- **Minimal J2C LOD support** — new `sl-proto::j2c` parses the codestream `SIZ`/
  `COD` markers (dimensions, components, decomposition levels) and ports the
  viewer's `calcDataSizeJ2C` byte-size estimate (`j2c::truncate_to_discard`), so
  the HTTP texture fetch can return the lower-resolution prefix for a requested
  discard level — header parsing only, **not** a pixel decoder.

New value types `AssetType` (with `to_code`/`from_code`/`get_asset_query_key`),
`ImageCodec`, `TransferStatus`, `Texture`, `Asset`. All wired as
`Command`/`SlCommand` variants through both runtimes (the HTTP fetches return
fully-formed session events over a binary-asset channel). Covered by `sl-proto`
unit tests (j2c header parse / discard-size / truncate) and four `lifecycle.rs`
tests (UDP `ImageData`+`ImagePacket` reassembly, `ImageNotInDatabase`,
`TransferRequest`→`TransferInfo`→`TransferPacket` reassembly with a `Params`
round-trip, and a `TransferInfo` failure). *Live-verified against the local
OpenSim via the new `asset_fetch` tokio example: the standard plywood texture
(`8955…`) came back as a 79 234-byte J2C over **both** the HTTP `GetTexture` cap
and the UDP `RequestImage` path, and a default sound (`ed12…`, 9 431 bytes) over
**both** `GetAsset` and a 16-packet UDP `TransferRequest` (last packet
`LLTS_DONE`). Test: local OpenSim — default textures/sounds suffice; no upload
needed.* Deferred: HTTP range requests (the LOD prefix is truncated client-side
rather than byte-ranged), AIS3 inventory-asset semantics, J2C/mesh decode, and
asset *upload* (#23).

*Update (2026-07): the **legacy UDP generic-asset transfer** half of this item
(`Session::request_asset` /
`TransferRequest`→`TransferInfo`→`TransferPacket` on the client side, and
`Event::AssetTransferStarted`) was **removed**. Modern Second Life always offers
the HTTP asset capability and the viewer only ever used the UDP transfer as a
fallback when it was absent, so the path was dead in practice (Second Life
refuses `SOURCE_ASSET` fetches, returning an error). Two corrections/additions
came with the removal: the generic-asset capability is named **`ViewerAsset`**
(not `GetAsset`) — both Second Life and OpenSim register it under that name, and
`FetchAsset` now selects it by that name — and a caching store, the **`sl-asset`
crate** (`AssetStore`: weak-reference sharing + single-flight + Firestorm-style
on-disk cache, no decode), was added as the opaque-asset counterpart of
`sl-texture` / `sl-mesh`. The legacy UDP **texture** path (`RequestImage`) and
the generated `TransferRequest`/`TransferInfo`/`TransferPacket` wire codec are
kept.*
