---
id: viewer-browser-view-popups-dropped
title: A target= link in any embedded browser but the browser window does nothing
topic: viewer
status: bugs
origin: found keying the web browser window (viewer-key-web-browser, 2026-11-04)
refs: [viewer-key-web-browser, viewer-keyed-floater-audit]
---

Context: [context/viewer.md](../context/viewer.md).

## Observation

The engine suppresses every popup — a `target="name"` / `target="_blank"`
link, a `window.open` — and leaves a `PopupRequest` on the surface for the
viewer to route (`sl-cef` `on_before_popup`). Only the web browser window
(`sl-viewer-media/src/web_floater.rs`, `sync_web_floater`) ever takes one.
The other embedded browser views never call `take_popup_request`, so a link
that opens a new window does nothing at all, and says nothing:

- the Search window's web tab (`sl-viewer-search/src/search.rs`);
- a resident profile's Web tab (`sl-viewer-people/src/avatar_profile.rs`);
- the media-settings preview (`sl-viewer-edit/src/edit_media.rs`).

The request also stays on the surface until the next popup overwrites it.

## Reference

`LLMediaCtrl::handleMediaEvent` handles `MEDIA_EVENT_CLICK_LINK_HREF` for
every media control the same way: try the URL as a SLURL, else
`LLWeb::loadURL(url, target, uuid)` — which opens (or reuses, for a named
target) a `web_content` window. So a link in search or a profile opens a
browser window.

## Fix shape

Route every UI (`SurfaceTrust::Viewer`) view's popup the way the browser
window does: an `OpenWebBrowser` with the page's target, once per request.
Probably one system in `browser_widget` over all views, with the browser
window's own popup routing folded into it. In-world media surfaces are a
separate question (the reference treats their clicks differently) and are out
of scope here.

Note from writing the e2e test for [[viewer-key-web-browser]]: a page's
load-time `window.open`, with no click behind it, never reached the viewer —
only a clicked `target=` link did. That matches what a browser lets a page do
and is not part of this bug.
