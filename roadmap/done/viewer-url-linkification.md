---
id: viewer-url-linkification
title: URLs in chat & other text contexts
topic: viewer
status: done
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

## Outcome (2026-08-09)

Shipped as two modules plus a parcel-name cache:

- `src/url_linkify.rs` — the pure, unit-tested matcher (a faithful port of the
  reference `llurlregistry::findUrl` leftmost scan, terminating-punctuation trim
  and `@`-email guard). Recognises `http(s)`/`ftp` URLs (trusted-SL-host vs.
  external, the internal/external browser split), the labelled `[url  text]`
  wiki form, SLURLs (`secondlife://` + `maps.secondlife.com`/`slurl.com`), the
  `secondlife:///app/...` agent/group/parcel/object entity links and the
  region/teleport/worldmap location apps. Every SLURL/app link also parses a
  **grid** (`secondlife://<Grid>/...`, `hop://`, `x-grid-location-info://`) so
  Aditi / OpenSim links resolve; a cross-grid entity shows its URL (its name is
  not in our caches). Each match carries a `LinkTarget`, a display label, an
  icon and a tooltip.
- `src/linkified_text.rs` — the reusable widget: segments a string and renders
  each run as a wrapping row of nodes, links being clickable boxes with a
  leading tinted icon (`assets/icons/link/{agent,group,location}.png`,
  white-mask SVG sources beside them) and the label text. Agent/group/parcel
  labels resolve in place from the caches;
  **hover shows the actual destination URL** (under a localised category line);
  a `Web` click opens the embedded browser (trusted SL host) or the system
  browser (external), and every click emits `LinkActivated` for the SLURL
  dispatcher.
- `src/parcel_names.rs` — a small `ParcelNames` cache (like `GroupsModel`) that
  folds the already-decoded `ParcelInfoReply` listing
  (`SlSessionEvent::ParcelDetails`) into a `ParcelKey → name` map, so parcel
  links resolve their name.

A gallery specimen (`linkified-text`) is registered and swept by the whole
`ui_test` matrix. `regex` was added as a viewer dep for the patterns.

**Rendering choice:** links are discrete inline nodes (each a real pickable box)
rather than glyph-rect hit-testing over one laid-out block — equally faithful to
the *matching* semantics and robust for the short chat / notice / profile
contexts. **Per-panel wiring stays in the sibling tasks**
([[viewer-load-url-body-links]], [[viewer-script-dialog-body-links]],
[[viewer-group-notice-body-links]], and the new
[[viewer-chat-sender-name-links]]) — this task delivered the shared layer,
gallery specimen and tests, not the panel retrofits. SLURL/entity **click
dispatch** remains [[viewer-slurl-parse-dispatch]]'s job (the widget only emits
`LinkActivated` + opens plain web links).
