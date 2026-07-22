---
id: viewer-destination-guide
title: Destination guide floater
topic: viewer
status: deferred
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-media-prim-browser]
---

Context: [context/viewer.md](../context/viewer.md).

The Destinations guide: a web-content floater rendering Linden's curated
destination-guide site (URL from login/grid info), with the page's
teleport links dispatched through the SLURL handler. Pure embedded-web
content — it rides the CEF work in [[viewer-media-prim-browser]] (in
progress in a parallel branch) plus a small SLURL bridge.

**Deferred**: Vintage drops the destinations toolbar button — not part of
the Vintage-parity target; revisit once the embedded browser is in and
the Vintage set is done.

Reference (Firestorm, read-only): `floater_destinations.xml`.
