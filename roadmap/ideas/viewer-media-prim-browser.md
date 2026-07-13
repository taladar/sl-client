---
id: viewer-media-prim-browser
title: Media-on-a-prim & embedded web browser
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-framework, viewer-streaming-audio]
---

Context: [context/viewer.md](../context/viewer.md).

Render web pages / video onto prim faces (media-on-a-prim) and provide a general
in-viewer browser (help, marketplace, currency, profile feeds).

**This is an integration task around an existing browser engine — we do not
build a browser.** The decisive, and likely hardest, fleshing-out step is
choosing the embedded-browser library and the "render to an offscreen surface →
blit into a Bevy texture → route input back" bridge. Survey candidates
(`wry` / system WebView, a CEF binding such as `cef` / `cef-ui`, Servo, or an
out-of-process plugin like Firestorm's own CEF media plugin) on offscreen-
rendering support, per-frame texture access, input injection, licensing, and
binary size.

Object media **metadata** is already ingested (`media.rs`) but never rendered;
this stub is the actual surface + browser. Parcel media controls overlap with
the streaming-audio nearby-media panel.

Reference (Firestorm, read-only): `llplugin/`,
`media_plugins/cef|libvlc|gstreamer`, `llmediactrl`, `llviewermedia`,
`llpanelprimmediacontrols`, `llmediadataclient`.

Builds on: `media.rs` metadata ingest.

Deps: [[viewer-ui-framework]], [[viewer-streaming-audio]].
