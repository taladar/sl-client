---
id: viewer-slurl-parse-dispatch
title: SLURL parsing & action dispatch
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-slurl-handling
refs: [viewer-world-map-floater, viewer-teleport-flow-progress]
---

Context: [context/viewer.md](../context/viewer.md).

Parse and dispatch Second Life URLs: `secondlife://<region>/x/y/z` region
SLURLs, `http://maps.secondlife.com/...` map links, and the
`secondlife:///app/...` app-command family (agent, group, object, teleport,
parcel, sharedmedia, etc.). Sources include chat, external OS protocol
registration (click a SLURL in a browser), the command line, and landmarks.

The two sides: a **parser** (reuse `sl-map-tools` SLURL / map-URL parsing where
it already exists) and a **dispatcher** that maps a parsed SLURL / app-command
to a registered handler (teleport, open profile, show parcel). Ship the registry
and dispatch mechanism now; the teleport handler routes to
[[viewer-teleport-flow-progress]] and the map / parcel handlers to
[[viewer-world-map-floater]] once those land — register them as the handlers
appear rather than blocking on them.

Reference (Firestorm, read-only): `llslurl`, `llurldispatcher`,
`llcommandhandler` (the `app/` handlers).

Builds on: `sl-map-tools` (SLURL / map-URL parsing).
