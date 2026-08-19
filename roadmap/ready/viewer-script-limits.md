---
id: viewer-script-limits
title: Script limits — parcel / attachment script resources
topic: viewer
status: ready
origin: Vintage-parity coverage audit (2026-07-22)
blocked_by: [viewer-ui-widget-scaffold]
refs: [api-g15]
---

Context: [context/viewer.md](../context/viewer.md).

The script-info floater: **parcel** script memory/URL usage (total, per
object, by owner — with return actions for landowners) and **my avatar**
attachment script usage — both over the resource-cost capabilities already
paired in [[api-g15]] (`LandResources`, `AttachmentResources` /
`ScriptResourceSummary` + details). This is the usage-list UI: summary
header, virtualized object list with owner filter, refresh.

Reference (Firestorm, read-only): `llfloaterscriptlimits`,
`floater_script_limits.xml`.

Builds on: [[api-g15]] resource-cost caps.

## Parity-audit addendum (2026-08-19)

The parity audit adds the context/pie entry points: wire the **Script
Info** slices (`script-info` placeholders on the object pie, the
avatar-self pie, and both attachment pies) to open the script-info
surface for the picked target. The avatar-target variant additionally
depends on the FS bridge (viewer-fs-bridge-protocol, blocked) — the
reference fetches avatar script info via the bridge; the object variants
do not need it.
