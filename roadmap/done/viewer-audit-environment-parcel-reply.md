---
id: viewer-audit-environment-parcel-reply
title: A parcel environment reply is accepted as the shared one
topic: viewer
status: done
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

## Fixed (2026-08-28)

`EnvironmentSource::of_reply` classifies a reply by its `parcel_id` — the `-1`
sentinel (`LLEnvironment::INVALID_PARCEL_ID`) is the whole region, every
non-negative id, `0` included, is one parcel's override — and the new
`EnvironmentState::ingest_reply` folds only a region-scoped reply into `shared`
and only a region-scoped reply clears `req_pending`. A parcel-scoped reply is
logged and dropped, so the region retry loop keeps running until the region's
own settings arrive, and "Use Shared Environment" restores those.

Five unit tests in `environment.rs` (previously zero for that file): the
`parcel_id` classification including the `0` boundary, a region reply becoming
the shared settings and ending the retry loop, a parcel reply not replacing the
region's settings, a parcel reply leaving the request outstanding, and
un-pinning a fixed sky restoring the region's environment after a parcel reply.

**Deliberately not addressed here:** the reference's `ENV_PARCEL` layer itself
— it records a parcel override *above* `ENV_REGION` and renders it
(`LLEnvironment::recordEnvironment`, `llenvironment.cpp:1874`), which this
viewer does not, and nothing here asks for a parcel-scoped environment yet
(`request_environment` sends `parcel_id: None`). Adding the layer without the
agent-parcel tracking and the request that populates it would be dead code; it
belongs with [[viewer-environment-personal-lighting]], whose scope already names
the region ⊂ parcel ⊂ local precedence chain, and
[[viewer-region-environment-panel]], which publishes parcel environments.
Two further reference guards are also out of scope and unimplemented: ignoring
a reply whose `region_id` is not the region the agent is on, and ignoring a
parcel reply for a parcel the agent has since left.
