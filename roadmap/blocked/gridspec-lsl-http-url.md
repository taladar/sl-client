---
id: gridspec-lsl-http-url
title: LSL library behaviour on each grid — outbound HTTP and in-world URLs
topic: gridspec
status: blocked
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
