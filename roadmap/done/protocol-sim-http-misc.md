---
id: protocol-sim-http-misc
title: Niche non-CAPS server channels — grid info, map tiles, helper URIs
topic: protocol
status: done
origin: user request (2026-07) — complete simulator protocol surface
points: 3
refs: [viewer-fake-grid, protocol-sim-terrain-raw-flows, idiomatic-xfer-framing-codec, viewer-fake-grid-udp-assets]
---

Context: [context/protocol.md](../context/protocol.md).

The SL protocol family has a handful of server-side HTTP surfaces that are
neither LLUDP nor seed-granted CAPS. Cover them server-side:

- the `get_grid_info` HTTP endpoint (the login-URI sibling the viewer and
  Firestorm's grid manager query);
- map-tile HTTP serving in the `map-server-url`/`MapTileURL` shape the
  viewer's world-map tile fetcher consumes;
- the login-response helper URIs (economy/currency XML-RPC helper
  endpoints) — enough surface that a client following the helper URI gets
  a well-formed reply;
- an audit item: verify `SimSession`'s server-side coverage of the legacy
  UDP Xfer/TransferRequest asset path, filing follow-up tasks for gaps
  found (the flow-level machines themselves are
  [[protocol-sim-udp-flows]]).

All sans-I/O builders/parsers in sl-wire where possible, consumed by
[[viewer-fake-grid]]'s HTTP glue.

**Done (2026-08-21).** All sans-I/O in `sl-wire`, served by `sl-fake-grid`:

- `xmlrpc.rs` — a generic XML-RPC codec over `Llsd` (calls, responses,
  faults, `method_name` peek); the login codec now shares its value
  bridge instead of private copies.
- `grid_info.rs` — `GridInfo` (ordered entries + typed accessors for the
  keys Firestorm's grid manager reads, `economy` over `helperuri`),
  `build_grid_info_xml` / `parse_grid_info_xml` (OpenSim-literal fixture
  round-trips byte for byte) and the XML-RPC `get_grid_info` pair. The
  script-only JSON variant (`json_grid_info`) has no viewer consumer and
  is out of scope.
- `map_tile.rs` — `MapTileRef` (`map-<zoom>-<x>-<y>-objects.jpg` build /
  parse, zoom 1–8).
- `economy_helper.rs` — typed request/response builders and parsers, both
  directions, for `getCurrencyQuote` / `buyCurrency` (`currency.php`) and
  `preflightBuyLandPrep` / `buyLandPrep` (`landtool.php`), Firestorm-literal
  fixture; `OpenSimExtras.currency_base_uri` added.
- `sl-fake-grid`: `GET /get_grid_info` + the XML-RPC method on `/`, tiles
  under the login URI (stock zoom-1 JPEG per region, `FakeGridBuilder::
  map_tile`, cache headers, HEAD), `EconomyConfig` policy + `FakeGrid::
  economy_events`, `GridIdentity` / `--grid-name` / `--grid-nick`; the
  login response now advertises `map-server-url` and `currency`, and the
  stock `SimulatorFeatures` carry the `OpenSimExtras` URLs. Covered by
  `tests/http_misc.rs` and the extended `client_end_to_end.rs` (the real
  client sees both). `Client::map_server_url` added to `sl-client-tokio`
  for parity with the Bevy `SlIdentity`.

**Audit outcome.** `SimSession`'s Xfer send/receive, transaction upload,
task-inventory serving, and `TransferRequest` responder are complete and pinned
in `SESSION_FLOW_COVERAGE`. Gaps filed: [[protocol-sim-terrain-raw-flows]] (no
`InitiateDownload` sender / named Xfer pull; `EstateOwnerMessage` `terrain`
silently dropped), [[idiomatic-xfer-framing-codec]] (framing duplicated four
times, no `sl-wire` codec, silent unknown-id aborts, implicit legacy `Asset`
source skip), [[viewer-fake-grid-udp-assets]] (the fake grid never answers
`XferRequested` / `TransferRequested` / `RequestTaskInventory`).
