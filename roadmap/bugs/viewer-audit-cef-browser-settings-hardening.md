---
id: viewer-audit-cef-browser-settings-hardening
title: An in-world media page runs on CEF's default browser settings
topic: viewer
status: bugs
origin: static code audit (2026-08-26), split out of
  [[viewer-audit-media-url-scheme-allowlist]]
points: 2
refs: [viewer-audit-media-url-scheme-allowlist]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-cef/src/chromium.rs:922` — every surface is created with

```text
let browser_settings = BrowserSettings {
    windowless_frame_rate: …,
    background_color: 0xFFFF_FFFF,
    ..BrowserSettings::default()
};
```

The same settings serve a **trusted UI panel** (the search tab, the web
floater, a profile page — `isolated: false`, so they see the grid's web-session
cookie) and an **in-world prim face** whose URL any object owner writes
(`isolated: true`). Only the request context differs; nothing else does.

[[viewer-audit-media-url-scheme-allowlist]] closed the scheme half of this — a
prim can no longer name `file://`, `data:` or `javascript:` — and scoped itself
to that deliberately. What is left is the per-surface *capability* set, which
CEF exposes on `BrowserSettings` (`javascript_close_windows`,
`javascript_access_clipboard`, `javascript_dom_paste`, `local_storage`,
`databases`, `webgl`, `image_loading`, …): the reference viewer's media plugin
turns several of these off for untrusted media, and the isolated surfaces here
turn none of them off.

Scope: split `BrowserSettings` by trust — one profile for `isolated: true`
(in-world content) and one for the trusted UI panels — driven off the field
`SurfaceConfig::isolated` already carries, with the reference's
`media_plugin_cef` settings as the model for which knobs to clear. Worth a
short live check afterwards that the search tab and web floater still behave
(they are the surfaces that legitimately need storage).
