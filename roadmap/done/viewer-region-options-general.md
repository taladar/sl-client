---
id: viewer-region-options-general
title: Region / Estate floater — region (general) tab
topic: viewer
status: done
origin: user request (2026-07-22) — coverage audit found the floater's
  first tab had no task (estate / terrain / debug already do)
blocked_by: [viewer-region-options-debug]
refs: [viewer-region-options-estate, viewer-region-options-terrain]
---

Done (2026-07-28): the About Region floater's **Region** tab — the identity
read-outs (name, type, owner as a clickable profile link, grid position), the
editable flag checkboxes, agent-limit / object-bonus fields, maturity combo, and
**Apply** (`SetRegionInfo`), plus Teleport-Home One / All. Estate-manager gated
(`is_estate_manager`) like its siblings.

Context: [context/viewer.md](../context/viewer.md).

The Region / Estate floater's **Region** tab: the general region
settings — block terraform / fly / damage, restrict pushing, allow
land resell / parcel join-divide, agent limit, bonus factor, maturity
rating, block parcel search, and the actions (send **region message**,
**restart region** with cancel, **teleport home one user / all
users**). The wire is complete (`RequestRegionInfo`, `SetRegionInfo`
(`RegionInfoUpdate`), `RestartRegion`, `SendEstateMessage`,
`TeleportHomeUser` / `TeleportHomeAllUsers`, `KickEstateUser`); this is
the tab UI on the floater shell [[viewer-region-options-debug]] stands
up, estate-manager gated like its siblings.

Reference (Firestorm, read-only): `llfloaterregioninfo.cpp`
(`LLPanelRegionGeneralInfo`), `panel_region_general.xml`.
