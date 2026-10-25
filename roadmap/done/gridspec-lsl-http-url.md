---
id: gridspec-lsl-http-url
title: LSL library behaviour on each grid — outbound HTTP and in-world URLs
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-http-url]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: `X-SecondLife-*` headers, throttle replies (status 499, body), URLs
per parcel. Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Outbound http and in-world urls.

## Fake grid

Large — [[server-lsl-lib-http-url]].

## Viewer

None.

## Done (2026-10-25)

- **Discover.** Three probes, `sl-lsl-runtime/probes/http/`: `outbound`
  (31 requests from a script to its own in-world URL, so it is both client
  and server and nothing leaves the grid: methods, mimetypes, custom and
  reserved headers, `HTTP_PRAGMA_NO_CACHE`, body lengths each way,
  `HTTP_BODY_MAXLENGTH`, statuses, and URLs no request can reach), `throttle`
  (a burst of 30 and a quiet burst of five) and `url` (`llGetFreeURLs`, a
  plain, a secure and a third URL, then requests from outside the grid).
  Complete on aditi and both OpenSim engines, which agree on every line.
  Second Life accepts 25 requests at once, OpenSim 4; Second Life cuts an
  inbound body to 2048 bytes, hides custom headers from `llGetHTTPHeader`,
  refuses `OPTIONS` and lower-case methods and answers `HEAD` with 400;
  OpenSim's URLs end in a slash (so `<url>/path` arrives as `//path`), it
  decodes the query, ignores `llSetContentType`, has
  `HTTP_PRAGMA_NO_CACHE` backwards, returns a key for a refused request
  that is then never answered, and denies `llRequestSecureURL` without an
  HTTPS listener.
- **Runner.** A `fetch` drive: the runner sends an HTTP request from
  outside the grid to the URL a probe prints, keeps the probe's lines while
  it waits, and notes the status, `Content-Type`, length, start of the body
  and seconds; the URL is kept out of the transcript (`url=<url>`). A
  `DEBUG_CHANNEL` note now says when the line was not fully audible. The
  per-probe deadline is three minutes (`http/outbound` runs about 85 s on
  OpenSim, where every refused request waits out its six seconds).
- **Local grid.** OpenSim refuses scripts any address on its own network,
  itself included, so `OpenSim.ini` now has
  `OutboundDisallowForUserScriptsExcept = 127.0.0.1:9000` and the
  `lsl_settings` fixture says so.
- **Not measured.** `HTTP_USER_AGENT`, `HTTP_ACCEPT`, `HTTP_EXTENDED_ERROR`
  (not in the vendored library table — [[server-lsl-library-table-refresh]]);
  `HTTP_VERIFY_CERT` against a bad certificate; an outbound request to a
  real outside server (what Second Life actually sends beyond what
  `llGetHTTPHeader` shows — a custom header, `Content-Type`, `Pragma`);
  HTML served to the owner's own viewer; URLs lost on a region restart, a
  crossing or an attachment's detach; and exhausting `llGetFreeURLs`.
- **Document.** `book/src/gridspec/lsl.md` § Outbound HTTP and in-world
  URLs; the `fetch` drive in `book/src/tools/lsl-probes.md`.
- **Fake grid.** Large, as planned: the rows are in
  [[server-lsl-lib-http-url]].
- **Viewer.** None.
