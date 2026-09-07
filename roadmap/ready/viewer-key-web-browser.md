---
id: viewer-key-web-browser
title: Decide whether the web browser is many windows or one with tabs
topic: viewer
status: ready
origin: split out of [[viewer-keyed-floater-audit]] (2026-09-07)
points: 2
refs: [viewer-keyed-floater-audit, viewer-profile-floater-single-instance]
---

Context: [context/viewer.md](../context/viewer.md).

`sl-client-bevy-viewer/src/web_floater.rs` (`"web-browser"`) is a singleton:
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
