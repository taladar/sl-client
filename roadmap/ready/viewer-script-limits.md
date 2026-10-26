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

## Measured (gridspec-avatar-render-info, 2026-10-21)

The attachment report is one document on both grids
(`book/src/gridspec/avatars.md` § Attachment resources): points by their LSL
names (`ATTACH_RIGHT_PEC` — map them to the point's display name), HUDs
listed, available memory `-1` (show as unlimited) and 38 URLs. OpenSim never
states an object's `memory` and its used memory is always 0 — show "not
reported", not 0 KB. Our client already reads it under either media label.

## Measured (gridspec-lsl-memory-limits, 2026-10-26)

What a script sees of the same figures (`book/src/gridspec/lsl.md` § Memory
and limits): Second Life counts an object's script memory as the sum of its
scripts' limits, running or not (65 536 a Mono script unless lowered, 16 384
an LSO one), OpenSim as 16 384 per running script. A floater that totals
attachment or parcel memory should expect exactly those figures.
