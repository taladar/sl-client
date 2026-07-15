---
id: viewer-region-options-estate
title: Region / Estate floater — estate tab
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-region-options-debug]
---

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater **estate** tab: covenant, access / allowed residents
and groups, estate managers, ban list, and region restart / sun controls — all
driven over the estate `EstateOwnerMessage`. Adds a tab to the floater shell
from [[viewer-region-options-debug]].

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`,
`llestateinfomodel`; the estate `EstateOwnerMessage`.

Builds on: `protocol-14` estate / region.

Deps: [[viewer-region-options-debug]].
