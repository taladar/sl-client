---
id: viewer-url-linkification
title: URLs in chat & other text contexts
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
---

Context: [context/viewer.md](../context/viewer.md).

A URL-registry / linkification system that recognises links in any text context
— nearby chat, IM, notifications, profiles, object descriptions — and renders
them clickable: plain `http(s)` URLs, SLURLs, and the `secondlife:///app/...`
entity links (agent / group / object / parcel), the last of which resolve to
display names and icons. Clicking dispatches the right action (open browser,
teleport, show profile) and hovering can preview.

This is the shared text-decoration layer every text-bearing panel consumes; it
feeds SLURL / app-command clicks into the SLURL dispatcher.

Reference (Firestorm, read-only): `llui/llurlregistry`, `llui/llurlentry`,
`llui/llurlmatch`, `llui/llurlaction`.

Deps: [[viewer-ui-framework]], [[viewer-slurl-handling]] (SLURL / app-command
click dispatch).
