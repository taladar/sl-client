---
id: viewer-load-url-body-links
title: Script web-page toast body — clickable URLs / SLURLs
topic: viewer
status: ready
origin: deferred from viewer-dialog-script-load-url (2026-07-29) — links parked
  like chat's, pending the linkification layer
blocked_by: [viewer-url-linkification]
refs: [viewer-dialog-script-load-url, viewer-slurl-parse-dispatch]
---

Context: [context/viewer.md](../context/viewer.md).

The script web-page request toast ([[viewer-dialog-script-load-url]]) renders
the `llLoadURL` **message** as plain text, and shows the target **URL** verbatim
as a plain (non-clickable) line so the user can vet it. The reference
`LoadWebPage` notification parses URLs / SLURLs in the message text, so a real
message's `http(s)` URLs and `secondlife:///app/...` SLURLs are clickable. Once
the shared linkification layer ([[viewer-url-linkification]]) lands, feed the
message through it so those runs render as links, exactly as nearby chat / IM,
the group notice ([[viewer-group-notice-body-links]]) and the script dialog
([[viewer-script-dialog-body-links]]) will — the click dispatch for a SLURL is
[[viewer-slurl-parse-dispatch]]'s job, not this one's.

The target URL line stays a deliberate plain read-out even after this lands: the
whole point of the toast is to vet the link before the **Load** button opens it,
so the URL must not itself be a one-click trap. This task only linkifies the
script's accompanying **message** prose.
