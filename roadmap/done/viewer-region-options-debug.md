---
id: viewer-region-options-debug
title: Region / Estate floater — region debug tab
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-region-options
blocked_by: [viewer-ui-widget-scaffold]
---

Done (2026-07-28): the Region / Estate ("About Region") floater shell
(`about_region.rs`, `AboutRegionPlugin`, opened from **World ▸ Region /
Estate…**, persistence-exempt, built once + updated in place) plus the **Debug**
tab — the disable scripts / collisions / physics toggles (new
`Command::SetRegionDebug` / `setregiondebug`) and Restart / Cancel-Restart
(`RestartRegion`, `-1` = cancel). The shell also stands up the Region, Terrain,
Estate, Covenant, Access, and placeholder Environment / Experiences tabs.

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate admin floater shell plus the **region debug** tab: terrain
raise / lower limits, object bonus, agent limits, and the region flags (fly,
build, damage, terraform, restrict push, etc.). This is the root of the region
floater — the terrain and estate tabs extend the shell it introduces.

Reference (Firestorm, read-only): `llfloaterregioninfo`, `llpanelregion*`; the
region-handshake flow.

Builds on: `protocol-14` estate / region.

Deps: [[viewer-ui-widget-scaffold]].

Note (2026-07-22): this floater is **subject-bound** — it opens on a
particular subject rather than persistent app state — so exempt it from
floater persistence (`floater_persist::FloaterPersistExempt` on the root,
as the avatar profile and item previews do): no restored rectangle, no
restored "open".
