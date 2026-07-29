---
id: viewer-group-notice-body-links
title: Group notice body — clickable URLs / SLURLs
topic: viewer
status: blocked
origin: deferred from viewer-group-notice-display (2026-07-29) — links parked
  like chat's, pending the linkification layer
blocked_by: [viewer-url-linkification]
refs: [viewer-group-notice-display, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

The group-notice toast ([[viewer-group-notice-display]]) renders the notice body
as **plain text**. The reference `panel_group_notify.xml` message editor sets
`parse_urls="true"`, so a real notice's `http(s)` URLs and
`secondlife:///app/...` SLURLs are clickable. Once the shared linkification
layer ([[viewer-url-linkification]]) lands, feed the notice body through it so
those runs render as links, exactly as nearby chat / IM will — the click
dispatch for a SLURL is [[viewer-slurl-parse-dispatch]]'s job, not this one's.

This is the same deferral chat's links carry: the toast's body node becomes a
link-decorated text context instead of a bare `Text`.
