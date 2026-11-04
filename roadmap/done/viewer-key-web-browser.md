---
id: viewer-key-web-browser
title: Decide whether the web browser is many windows or one with tabs
topic: viewer
status: done
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 2
refs: [viewer-keyed-floater-audit, viewer-profile-floater-single-instance,
  viewer-browser-view-popups-dropped]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-media/src/web_floater.rs` (`"web-browser"`) is a singleton:
`WebFloaterUi` holds one root, one embedded view, one address field, one
history. Every `OpenWebBrowser` — a menu item, a link routed from another
floater, a SLURL dispatch — lands in that one window, replacing whatever page
was open.

Unlike the other entries in [[viewer-keyed-floater-audit]] this one is a
**decision before an implementation**, because "one subject per window" is not
obviously the right shape for a browser:

- **Keyed windows** (one per opened URL) fall straight out of the scaffold and
  need no new UI. They also multiply the embedded browser process/view per
  window, which is the expensive part.
- **Tabs in one window** is what every browser does and what a resident
  expects; the reference viewer keeps `LLFloaterWebContent` as a *registered
  instance per key* with tab-like reuse, so its answer is closer to this than
  to a plain singleton.

## What to do

1. Read the reference (`llfloaterwebcontent.cpp`, its `Params::id` /
   `preferred_media_size` handling and how `LLFloaterReg` keys it) and write
   down what it actually does — the audit currently records only "arguably
   tabs".
2. Weigh the cost of a second embedded view (CEF process, memory, focus
   handling) against the keyed scaffold's simplicity. That cost is the only
   real argument against keying, so measure it rather than assuming it.
3. Take the decision and record it in the audit either way. **A window that
   stays a singleton is a finding worth writing down, not a no-op.**
4. Implement the chosen shape: either the keyed conversion (key by the target
   URL or by the caller's purpose — a help window and a profile web tab are
   different subjects), or the tab strip inside the one window.

## How to verify

Open two different pages from two different sources (the menu and a link in a
profile). Whichever shape is chosen, both pages must be reachable at once, each
with its own history and address, and closing one must not disturb the other.

## Outcome (2026-11-04) — keyed windows, as the reference keeps them

**What the reference does.** `LLFloaterWebContent` is neither a singleton
nor a tab strip. It is a registered instance per key
(`LLInstanceTracker<…, std::string>` on its `id`), and
`LLWeb::loadURLInternal` is how nearly every caller opens it (the profile web
link, the login panel's links, Help, the toolbar button). It passes no id, so
`preCreate` makes a fresh UUID, and **every page opens a new window**.
`matchesKey` reuses a window only when the request names a `target` other
than `_blank` and a window with that target is open. That is how a page's
`target="name"` link (`LLMediaCtrl`, `MEDIA_EVENT_CLICK_LINK_HREF` →
`LLWeb::loadURL(url, target)`) keeps landing in one window.
`WebContentWindowLimit` (default 5, 0 = none) closes the least recently opened
window before a new one would go over it. `preferred_media_size` only sizes a
window's content; it has nothing to do with keying. So the audit's "arguably
tabs" was wrong: the reference's answer is plain keyed windows.

**What a window costs** (measured: an e2e run with each step held for 12 s,
summed PSS of the `sl-cef-helper` processes, two backends):

| Windows open | Helper processes | Helper PSS |
| --- | --- | --- |
| 0 | 5 / 7 | 107 / 129 MiB |
| 1 | 7 / 9 | 172 / 191 MiB |
| 2 | 9 / 11 | 315 / 322 MiB |
| 3 (same site as the 2nd) | 10 / 12 | 326 / 334 MiB |

A window on a new site costs about one renderer process, roughly 60–140 MiB
depending on the page (each fresh window starts on the home page, which is
heavy). A window on a site that is already open shares that site's renderer
and costs about 10 MiB. The CEF browser side in the viewer process was not
broken out. The limit of five bounds the total, as it does in the reference.
A tab strip would cost the same, since each tab is its own browser too, so
the cost is no argument for tabs.

**Decision: keyed windows.** That is what the reference does, the scaffold
gives it to us without new UI, and it costs no more than tabs would.

**What landed.**

- `OpenWebBrowser` carries a `WebTarget` (`New` / `Named`), and
  `OpenWebBrowser::new_window` is what every existing opener uses.
  `web_window_key` keys a window as `new-<seq>` or `target-<name>`, with
  prefixes that keep a page from naming a key that collides with a fresh
  window's.
- Each window's parts are a `WebWindow` component on its root. The toolbar
  buttons and the address bar find their own window with `FloaterHost`, and
  the toolbar no longer goes through a window-less `UiAction`.
- `PopupRequest { url, target }` (in `sl-media`): the CEF hook now passes on
  the page's target frame name, and the browser window opens a popup through
  `OpenWebBrowser` with that target, so it no longer replaces the page that
  asked for it. Popups from the other embedded views are still dropped:
  [[viewer-browser-view-popups-dropped]].
- `WebContentWindowLimit` (`[media]`, default 5) closes the oldest window.
- A request for a window whose surface is not created yet changes the page
  that window will open on, where it used to be dropped silently. Two
  requests for one named window in the same frame open it once, on the later
  page.
- Content ▸ Web Browser opens a new window on the home page each time, like
  the reference's toolbar, instead of toggling a singleton. It has no check
  mark any more.
- Tests: five unit tests in `web_floater` (two pages give two windows, a
  named target is reused, one frame with two requests for one name, the sixth
  window closes the oldest, a refused URL opens nothing), and the e2e
  `every_page_keeps_a_browser_window_of_its_own`. That test opens two windows
  from the menu, each keeping the address typed into it, clicks a page's
  `target="stagewin"` link to get a third window under that key, and closes
  the second while the other two keep their pages.
