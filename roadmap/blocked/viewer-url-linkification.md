---
id: viewer-url-linkification
title: URLs in chat & other text contexts
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-ui-text-foundation]
---

Context: [context/viewer.md](../context/viewer.md).

A URL-registry / linkification system that recognises links in any text context
— nearby chat, IM, notifications, profiles, object descriptions — and renders
them clickable: plain `http(s)` URLs, SLURLs, and the `secondlife:///app/...`
entity links (agent / group / object / parcel), the last of which resolve to
display names and icons for the visible label. Each rendered link carries its
URL as the target and hovering can preview; dispatching the click (open browser
for `http(s)`, or the SLURL action) is the consumer's job.

This is the shared text-decoration layer every text-bearing panel consumes: it
turns runs of text into clickable links (visible text, URL target). What a click
then *does* for a SLURL is [[viewer-slurl-parse-dispatch]]'s concern, not this
one's.

Reference (Firestorm, read-only): `llui/llurlregistry`, `llui/llurlentry`,
`llui/llurlmatch`, `llui/llurlaction`.

Deps: [[viewer-ui-text-foundation]] (the text-run decoration layer). Independent
of [[viewer-slurl-parse-dispatch]] — that system wires SLURL actions to their UI
targets from any source; this one only renders text as clickable links.
