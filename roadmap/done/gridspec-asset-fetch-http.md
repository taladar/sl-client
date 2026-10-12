---
id: gridspec-asset-fetch-http
title: HTTP asset fetch details on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-12)

Measured and written up in `book/src/gridspec/assets.md`.

- **Discover.** A new case, `asset-http`, rather than more metrics on the
  three fetch cases (they drive the client's stores, which hide the HTTP):
  it asks `GetTexture`, `GetMesh`, `GetMesh2` and `ViewerAsset` itself, with
  a client that follows no redirect, about one asset of each: the whole
  asset and its headers; fifteen `Range` headers; the entity tag and the
  date sent back; a `HEAD`, a `POST`, another `Accept`, `gzip`, the URL
  without its slash; and what is not there — an unknown id, the nil id, no
  id, no query, an unknown key, an asset of another class. Four runs on
  aditi and five on OpenSim, held to 148 measured answers.
- **Findings.** Neither grid ever answers `416`: **a range that starts at or
  past the end of an asset is a `200` with the whole asset** on both. Second
  Life's capabilities are on another host, over plain `http` — **all four
  the same URL, that host's bare root**, with no token — and state an
  `ETag` and a `Last-Modified` (the date honoured with `304`, the tag not),
  no `Cache-Control`, and `Content-Encoding: binary/octet-stream`; OpenSim
  states nothing for a cache. OpenSim's range parser reads a suffix as the
  first eleven bytes, `bytes=0-0` and two ranges as no range, and a
  backwards range as one byte; Second Life answers two ranges with a
  multipart body. OpenSim answers a `POST` as a `GET`, a request that names
  no asset with `400`, a missing asset with `404`, and serves an asset
  whatever class the request names. No redirect and no transient status on
  an asset either grid serves.
- **Aditi's missing-asset code is not Second Life's.** Aditi answers `503`
  ("DNS failure", from its proxy) for a missing asset, for a malformed
  request **and for the four body parts and clothes the test avatar wears**:
  its proxy cannot reach its store for anything it has not in hand. What
  the main grid answers for a missing asset cannot be measured from here.
- **Fake grid.** `ImitatedGrid::asset_http_policy` → `sl_proto`'s
  `AssetHttpPolicy`: range reading per flavour, the whole asset past the end
  on both, `HEAD`, validators and `304` on the Second Life flavour, `POST`
  and `400` on the OpenSim one, `ViewerAsset` media types by class. The
  Second Life flavour has an asset host of its own (`asset_host`): a second
  listener whose bare root is all four capabilities, session-free. The case
  runs offline on both flavours. **Not imitated on the Second Life
  flavour**, each for a stated reason: the `503` for a missing asset (the
  fake answers `404`) and the `Content-Encoding` header. A `FakeSl` run is
  not held to those rows.
- **Viewer.** The stores never read a `206`'s `Content-Range`, so a
  full-resolution texture's last request started at the asset's end and was
  answered with the whole asset: **every full-resolution texture was
  downloaded twice, on both grids**. `FetchChunk::total` carries the stated
  length from all six fetchers (both runtimes) and the texture store and the
  mesh store's header probe stop at it (a unit test fails without the fix).
  Retrying a `503` was already there at two levels (the fetcher's eight
  tries, the viewer's bounded re-issue) and is unchanged.
- **Not measured.** A missing asset on the main grid; a busy asset host;
  the appearance service; connection limits; the UDP roads; OpenSim's
  texture format conversion.

## Known already

Fetches are green on both grids; ViewerAsset 503s happen on aditi; asset caps
live at the root region on SL.

## Discover

Extend the fetch cases to record status codes, ranges (206 vs 200), redirects,
cache headers, missing-asset code.

## Document

`book/src/gridspec/assets.md`.

## Fake grid

Small — status-code and range fidelity per flavour.

## Viewer

Retry on 503, partial ranges.
