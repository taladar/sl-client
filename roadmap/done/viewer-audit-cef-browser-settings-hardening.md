---
id: viewer-audit-cef-browser-settings-hardening
title: An in-world media page runs on CEF's default browser settings
topic: viewer
status: done
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

## Resolution (2026-09-15)

**Trust is now a named thing a call site has to say out loud.**
`SurfaceConfig::isolated: bool` is gone; in its place is
`sl_media::SurfaceTrust` — `Viewer` (a search tab, the web floater, a
viewer-authored page) or `InWorld` (media-on-a-prim, parcel media, and now the
profile Web tab — see below). One value, two readers: `is_isolated()` still
picks the request context, and the new `chromium::browser_settings` picks the
capability set. The bool named only the storage half, which is how the
capability half came to be nobody's decision.

`browser_settings` follows the reference's `media_plugin_cef` in what it
*keeps*: JavaScript, image loading, remote fonts and WebGL stay on for both
profiles, because an in-world page without them is not media any more (the
reference sets `webgl_enabled = true` outright). The hardening is spent on the
capabilities that reach back *out of* the page, and `InWorld` loses all five:

- `javascript_close_windows` — a page must not close a surface the viewer owns;
- `javascript_access_clipboard` and `javascript_dom_paste` — the sharpest of
  the set, since a clipboard read hands whatever the user last copied (a
  password, a chat line) to the object owner's server on the next fetch;
- `local_storage` and `databases_deprecated` — an in-world surface already gets
  a fresh in-memory request context, so storage is only a place to accumulate
  state the user cannot see or clear.

Both profiles state every knob explicitly rather than leaving the trusted one
at `State::DEFAULT`, so the contrast reads in the source instead of in
Chromium's build flags. `getUserMedia` needed nothing: CEF keeps media capture
off unless `--enable-media-stream` is passed, and this backend never passes it.

Unit-verified in `sl-cef` (5 tests, all previously green against
`State::DEFAULT` everywhere): the five capabilities an `InWorld` page loses,
the four it keeps, the whole set a `Viewer` panel keeps, that trust disturbs
neither the paint rate (clamped 1–60) nor the background colour, and that
`InWorld` is exactly the isolated case.

### Beyond the filed scope

The audit item scoped itself to "driven off the field `SurfaceConfig::isolated`
already carries" — i.e. keep each surface's existing trust and only add
capabilities. One surface was **re-classified** instead:

**The profile Web tab now loads at `InWorld` trust.**
[[viewer-audit-media-url-scheme-allowlist]] closed with this recorded as
unsettled: the panel spawned with `isolated: false`, i.e. in the shared context
carrying the grid's web-session cookie, for a URL any avatar types into their
homepage field — and whether the reference does the same needed checking before
it could be called a defect. It was checked. The reference's
`LLPanelProfileWeb` does inject the OpenID cookie (`postBuild` →
`getOpenIDCookie`), but its one `LLMediaCtrl` serves *two* pages: the grid's own
web-profile page (`mURLWebProfile`, which needs the session) and the avatar's
homepage (`mURLHome`, which inherits the cookie because it shares the control).
Ours only ever loads the homepage field. There is nothing in our panel the grid
session is for, so it is in-world content and is now treated as such —
isolated, and without the clipboard.

The search tab (the grid's own search page) and the web floater (the user's own
address bar, the reference's trusted `LLFloaterWebContent`) keep `Viewer`.

The profile panel's trust is not pinned by a test: `build_web_tab` needs a whole
`ProfileUi` (≈20 entity fields, no constructor) to reach, and the assertion
would only read back a literal in the same file. The enum forcing every call
site to name its trust, plus the reasoning recorded at that call site, is what
carries it.
