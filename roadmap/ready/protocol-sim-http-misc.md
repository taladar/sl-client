---
id: protocol-sim-http-misc
title: Niche non-CAPS server channels — grid info, map tiles, helper URIs
topic: protocol
status: ready
origin: user request (2026-07) — complete simulator protocol surface
points: 3
refs: [viewer-fake-grid]
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
