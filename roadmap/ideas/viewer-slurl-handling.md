---
id: viewer-slurl-handling
title: SLURL handling
topic: viewer
status: ideas
origin: reference-viewer feature-cluster survey (2026-07)
blocked_by: [viewer-teleport-flow, viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

Parse, resolve, and act on Second Life URLs: `secondlife://<region>/x/y/z`
region SLURLs, `http://maps.secondlife.com/...` map links, and the
`secondlife:///app/...` app-command family (agent, group, object, teleport,
parcel, sharedmedia, etc.). Sources include chat, external OS protocol
registration (click a SLURL in a browser), the command line, and landmarks.

The two sides: a **dispatcher** that maps a SLURL / app-command to an action
(teleport, open profile, show parcel), and the **routing** from where SLURLs
appear. Reuse `sl-map-tools` SLURL parsing where it already exists.

Reference (Firestorm, read-only): `llslurl`, `llurldispatcher`,
`llcommandhandler` (the `app/` handlers).

Builds on: `sl-map-tools` (SLURL / map-URL parsing).

Deps: [[viewer-teleport-flow]] (teleport action), [[viewer-ui-widget-scaffold]].
Independent of [[viewer-url-linkification]]: this dispatches SLURL actions to
their UI targets regardless of where the SLURL came from (chat, an external
browser, the command line), while linkification only turns text into clickable
links.
