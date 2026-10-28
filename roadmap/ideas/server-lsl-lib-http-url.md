---
id: server-lsl-lib-http-url
title: Library tranche — outbound HTTP and in-world URLs
topic: server
status: ideas
origin: LSL-on-the-fake-grid audit (2026-09-20)
blocked_by: [server-lsl-vm-execution, server-world-determinism-contract,
  gridspec-lsl-http-url]
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

## Measured (gridspec-lsl-http-url, 2026-10-25)

The rows are in `book/src/gridspec/lsl.md` § Outbound HTTP and in-world URLs,
from the `http/*` probes on aditi and both OpenSim engines; the fake grid
should take each flavour's column. The parts that shape the implementation:

- **The URL** a flavour grants: Second Life's `…/cap/<key>` with no closing
  slash (and an `https://…:12043` one for `llRequestSecureURL`), OpenSim's
  `…/lslhttp/<key>/` with one (and `URL_REQUEST_DENIED` for a secure one
  without an HTTPS listener). `x-script-url` is the URL without that slash;
  OpenSim decodes `x-query-string`, Second Life does not.
- **What `llGetHTTPHeader` gives**: on Second Life only the `x-*` headers
  and `user-agent` — never a custom header, `content-type` or `pragma`; on
  OpenSim every header.
- **The headers a script's request carries**: `X-SecondLife-Shard`
  (`Testing` / `OpenSim`), `-Object-Name`, `-Object-Key`, `-Owner-Name` (the
  legacy name), `-Owner-Key`, `-Region` (`Name (x, y)`, the corner in
  metres), `-Local-Position`, `-Local-Rotation`, `-Local-Velocity` in
  parentheses; the user agent `Second-Life-LSL/<version>
  (https://secondlife.com)` or none; OpenSim's inverted `Pragma: no-cache`.
- **Refusals**: Second Life refuses with `NULL_KEY` and a `DEBUG_CHANNEL`
  line (each text is in the table); OpenSim returns a key that is never
  answered, after its own line.
- **Lengths**: an inbound body over 2048 bytes is cut to 2048 on Second
  Life; an answer is cut at `HTTP_BODY_MAXLENGTH` (2048 by default, at most
  16384 on Second Life, unbounded on OpenSim), with `[HTTP_BODY_TRUNCATED,
  <n>]` metadata on Second Life only.
- **Timeouts and errors**: an unanswered request is 504 `Timed out waiting
  for response from internal server.` (26 s) or 500 `Script timeout` (25 s);
  a released URL 404; `HEAD` 400 and `OPTIONS` answered by the simulator on
  Second Life; `llSetContentType` honoured on Second Life only.
- **The throttle**: Second Life accepts 25 requests at once from a prim and
  shouts `Too many HTTP requests too fast.` for each refusal unless
  `HTTP_VERBOSE_THROTTLE` is off; OpenSim's defaults accept 4 and refuse
  silently. A stated tick count in the fake grid's model, per the
  determinism rule.
- **`HTTP_USER_AGENT`, `HTTP_ACCEPT`, `HTTP_EXTENDED_ERROR`** (Second Life
  only; OpenSim does not compile them —
  `gridspec-lsl-refreshed-table-probe-steps`, 2026-10-27): the user agent
  is appended to the default after a space, the last one given wins, and a
  value with a space, an empty one or one of 306 characters is a refusal
  (not a script error); `HTTP_ACCEPT` takes one MIME type (a list, `*/*` and
  an empty value are refused) and an answer of another type comes back as
  415 `Unsupported or unknown Content-Type.`; with `HTTP_EXTENDED_ERROR` on,
  a refusal returns a key and an `http_response` with an RFC 7807 body
  (471 URL, 472 parameter, 473 custom header, 405 method — texts in the
  chapter), still with the `DEBUG_CHANNEL` line, and the option list is read
  in order, so a bad option before the flag is refused as without it.
  `DEBUG_CHANNEL` complaints flood out after about eleven in thirty seconds
  (`Too many errors... dropping further messages until the flood stops`).

## Measured (gridspec-lsl-throttles, 2026-10-28)

How the throttle lets go (`book/src/gridspec/lsl.md` § HTTP requests: how
the throttle lets go): Second Life allows 25 requests in 20 s and **counts the
refused ones**, so a script retrying every half second is refused for as long
as it keeps trying; 20 s of silence (not 15) give the whole 25 back. OpenSim
refills its bucket of 3 by one a second and refusals do not drain it. The
throttle per flavour is [[server-lsl-forced-delays-per-flavour]].
