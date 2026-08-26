---
id: viewer-audit-media-url-scheme-allowlist
title: Parcel media URLs reach CEF and GStreamer with no scheme allowlist
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 3
---

Context: [context/viewer.md](../context/viewer.md).

`sl-media/src/lib.rs:517` — `classify_url` matches `STREAM_SCHEMES`, then falls
through to `MediaKind::Web` for **everything** unrecognised, including
`file://`, `data:` and `javascript:`. There is no filter downstream either:
`SurfaceConfig::initial_url` (`:113`) and `MediaSurface::navigate` (`:308`) take
a bare `String`, `sl-cef/src/chromium.rs:563` passes it to CEF with an
unhardened `BrowserSettings::default()` (`:922`), and `sl-gst/src/stream.rs:142`
sets it as `playbin3`'s `uri` (GStreamer's `uridecodebin` opens `file://`
happily — `sl-gst/src/surface.rs:782` even tests with a `file://` URL).

Parcel and prim media URLs are supplied by any land or object owner.

**Scoped honestly:** Chromium's `allow_file_access_from_file_urls` defaults to
false, so a `file://` page cannot read *other* local files and exfiltrate them.
The real impact is that a land or object owner can make your viewer open and
render a local file on a prim face, visible only to you. This is a hardening
gap, not a data-exfiltration hole.

Scope: a scheme allowlist in `sl-media`, made unbypassable with a
`ValidatedMediaUrl` newtype so the type system carries the evidence — matching
the workspace's existing typed-newtype convention. The 4 existing `classify_url`
tests are all happy paths; the table test to add asserts `file://`, `data:`,
`javascript:`, `about:` and `chrome://` are each **rejected**.
