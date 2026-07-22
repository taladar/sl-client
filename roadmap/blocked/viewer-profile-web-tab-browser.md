---
id: viewer-profile-web-tab-browser
title: Profile Web tab — render the feed in the embedded browser
topic: viewer
status: blocked
origin: user request (2026-07-22), while shipping viewer-social-profiles
blocked_by: [viewer-media-prim-browser]
refs: [viewer-social-profiles]
---

Context: [context/viewer.md](../context/viewer.md).

The profile floater's **Web tab** ([[viewer-social-profiles]]) currently
shows (and, for one's own profile, edits) the `profile_url` string only. The
reference renders the URL's page in an embedded `web_browser` control
(`panel_profile_web.xml`, `LLPanelProfileWeb`) with a load-status line —
navigation driven by code, no visible URL bar.

Once [[viewer-media-prim-browser]] lands the CEF engine and the browser
widget, upgrade the tab: keep the URL edit for one's own profile, render the
page below it for any profile, and add the reference's load-time status
string. The same widget then also unlocks the search / marketplace / L$
floater surfaces that task catalogues.
