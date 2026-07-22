---
id: viewer-hover-tooltips
title: In-world hover tooltips (object / avatar / land inspectors)
topic: viewer
status: ready
origin: user request (2026-07-22) — only llSetText hover *text* had a
  task (viewer-hover-text); the hover *tooltip* had none
refs: [viewer-hover-text, viewer-name-tags-billboard-render]
---

Context: [context/viewer.md](../context/viewer.md).

The reference's **hover tips**: rest the pointer on something in-world
for a moment (~0.5 s) and a small info box appears at the cursor —

- **objects**: name, description, owner (creator for the extended
  tip), and the affordance hints the reference appends (touch /
  sit / buy price / pay, from the click action and flags). The data
  arrives by firing `RequestObjectPropertiesFamily` for the hovered
  object on a debounce (the reference's `LLToolPie::handleHover` →
  `LLSelectMgr` flow) — command and reply are already on the wire.
- **avatars**: the resolved name (and display name once
  [[viewer-name-tags-display-names]] lands) — the same sources the
  name tags read.
- **land**: the parcel name / owner summary when nothing pickable is
  hit (the reference gates this behind "Show land tooltips"; held
  parcel data covers it).

Needs a small shared tooltip surface (anchored at the cursor,
delay + move-away dismiss) — the UI piece menus/floaters do not give
us yet — plus the hover debounce over the existing world pick
(mesh-accurate avatar pick + object ray cast). RLV info-hiding
interactions belong to [[viewer-rlv-enforce-info-hiding]].

Reference (Firestorm, read-only): `lltooltip.cpp`, `lltoolpie.cpp`
(`handleTooltip...`), `llinspectobjects/avatar` (the modern
inspectors; Vintage keeps classic tips).
