---
id: gridspec-avatar-render-info
title: AvatarRenderInfo and AttachmentResources on each grid
topic: gridspec
status: ready
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

OpenSim registers `AttachmentResources`, has no `AvatarRenderInfo`; no client or
server code for the latter.

## Discover

`sl-repl --script` cap probes on both grids; Firestorm's
`llavatarrenderinfoaccountant.cpp`; a second avatar for others' reports.

## Document

`book/src/gridspec/avatars.md` § Render info.

## Fake grid

Small once the protocol exists — [[protocol-avatar-render-info]].

## Viewer

Report own complexity and show others' on SL; nothing on OpenSim.
