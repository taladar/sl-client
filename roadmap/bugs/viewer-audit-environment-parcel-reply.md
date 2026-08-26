---
id: viewer-audit-environment-parcel-reply
title: A parcel environment reply is accepted as the shared one
topic: viewer
status: bugs
origin: static code audit (2026-08-26)
points: 2
---

Context: [context/viewer.md](../context/viewer.md).

`sl-viewer-world-scene/src/environment.rs:310` — `ingest_environment` accepts a
**parcel** environment reply as if it were the shared/region one and clears
`req_pending`.

Two consequences: a parcel override becomes what "Use Shared Environment"
restores, and clearing `req_pending` cancels the region retry loop, so the real
shared environment is never re-requested.

Fix: discriminate on the reply's scope and only fold a region-scoped reply into
the shared settings.
