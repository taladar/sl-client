---
id: server-lsl-lib-http-url
title: Library tranche — outbound HTTP and in-world URLs
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-determinism-contract]
refs: [server-lsl-lib-task-inventory]
---

Context: [context/lsl.md](../context/lsl.md).

The one tranche that wants to leave the process, on a grid whose whole
value is that it does not.

- **Outbound**: `llHTTPRequest` with its `HTTP_*` option list (method,
  mimetype, body max length, verify cert, verbose throttle, custom
  headers, pragma no-cache), and the
  `http_response(key request_id, integer status, list metadata, string body)`
  event. Real grids add their own headers
  (`X-SecondLife-Object-Name`, `-Owner-Key`, `-Region`, `-Local-Position`,
  …) which server operators authenticate against, so a faithful
  implementation sends them too — and they are the part a test can
  assert without any network at all.
- **Inbound**: `llRequestURL`, `llRequestSecureURL`, `llReleaseURL`,
  `llGetFreeURLs`, `llHTTPResponse`, `llGetHTTPHeader`, and the
  `http_request(key id, string method, string body)` event. OpenSim's
  is `CoreModules/Scripting/LSLHttp`; the fake grid already runs an HTTP
  service for CAPS, so minting a per-script URL on it is a small
  addition — and an *in-process* one, which is the interesting case: a
  script can call its own region's URL with no network.

The determinism rule ([[server-world-determinism-contract]]) decides the
shape:

- **Outbound HTTP is off by default.** A scenario injects a responder —
  a trait with one method, `respond(request) -> response` — and the
  default responder answers every request with a stated failure. A
  scenario that wants a canned API mounts a table of URL → response.
  Nothing in a `cargo test` run touches the network.
- **A real-network responder exists but is opt-in**, for the case where
  the fake grid is being used interactively to develop against a live
  service, and it is loudly non-deterministic.
- Response *timing* is a stated number of ticks, not a measured
  round-trip.

Acceptance: a script that requests a URL, receives an `http_request`
from a test client hitting it, and answers with `llHTTPResponse`, end to
end in one `cargo test` with no socket outside loopback; the SL request
headers present and asserted; and an outbound request with no injected
responder failing cleanly rather than hanging.
