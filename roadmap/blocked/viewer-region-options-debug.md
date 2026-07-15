---
id: viewer-region-options-debug
title: Region / Estate floater — region debug tab
topic: viewer
status: blocked
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-ui-widget-scaffold]
---

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate admin floater shell plus the **region debug** tab: terrain
raise / lower limits, object bonus, agent limits, and the region flags (fly,
build, damage, terraform, restrict push, etc.). This is the root of the region
floater — the terrain and estate tabs extend the shell it introduces.

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`; the
region-handshake flow.

Builds on: `protocol-14` estate / region.

Deps: [[viewer-ui-widget-scaffold]].
