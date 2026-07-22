---
id: viewer-minimap-interactions
title: Minimap interactions — clicks, double-click teleport, context menu
topic: viewer
status: blocked
origin: user request (2026-07-22); split from viewer-minimap
blocked_by: [viewer-minimap, viewer-minimap-avatar-dots]
refs: [viewer-double-click-teleport, viewer-world-map-tracking-teleport]
---

Context: [context/viewer.md](../context/viewer.md).

Everything the minimap does when you click it. Reference facts
(Firestorm `llnetmap.cpp` handlers + `menu_mini_map.xml`, researched
2026-07-22):

## Clicks

- Single left-click: no action in the reference (a TODO to select the
  avatar in the radar — a candidate for us to do better once
  [[viewer-avatar-radar]] exists). SHIFT-drag pans (base task).
- **Double-click** — `FSNetMapDoubleClickAction` (default **2**):
  0 = nothing, 1 = open the world map, 2 = **teleport**
  (`teleportViaLocationLookAt` at the clicked point). For 1 and 2 it
  first sets a tracking beacon at the clicked spot (world-map
  `trackLocation`) unless already tracking. Use the same
  teleport/tracking backend as [[viewer-world-map-tracking-teleport]]
  and [[viewer-double-click-teleport]] — one backend, three surfaces
  (user request: shared data/plumbing, not three implementations).
- The active track target (avatar / landmark / location) renders on the
  map as a `MapTrackColor` (red) dot, with an edge arrow when
  off-screen.

## Context menu (right-click)

Avatar-context items (shown only when right-clicking a dot; multi-dot
variants when several avatars are within pick radius):

- View Profile (single) / View Profiles submenu (one entry per avatar)
- Add to Contact Set (single & multiple)
- Cam (zoom camera to avatar), Face towards avatar
- Mark submenu: red/green/blue/purple/light-yellow dot mark; Clear
  Mark(s); Clear All Marks (feeds the dot colours —
  [[viewer-minimap-avatar-dots]])
- More Options submenu (the full avatar action set: Add/Remove Friend,
  IM, Call, Map, Share, Pay, Offer/Request Teleport, Teleport To,
  Invite To Group, Get Script Info, Block/Unblock, Report, Freeze,
  Parcel Eject, Estate Kick/Teleport Home/Ban, Derender,
  Derender + Blacklist) — route these to the same shared avatar-action
  layer the profile/people UIs use; do not reimplement per menu
- Start Tracking (track that avatar); Stop Tracking (shown only while
  tracking)

Map items (always):

- Zoom submenu with checks: Very Close (1024) / Close (256) /
  Medium (128, default) / Far (32)
- Show submenu: Objects (`MiniMapObjects`); Physical / Scripted /
  Temp-on-rez objects (`FSNetMap*`, enabled only when Objects is on);
  Property Lines (`MiniMapShowPropertyLines`); Parcels for Sale
  (`MiniMapForSaleParcels`, enabled when property lines on)
- North at top / Camera at top (the `MiniMapRotate` pair, checks)
- Auto-center map (check); Re-center map (enabled only when off-centre)
- Chat Distance Rings submenu (master + whisper/chat/shout toggles)
- About Land (select parcel at click, open About Land; enabled only on
  a valid parcel), Place Profile, World Map (RLV can disable the
  location/world-map items)

Menu shape: the reference uses a regular context menu, not a pie menu —
implement it as one. (If any part is ever promoted to a pie menu, the
pie-menu convention applies: a committed test pinning each action's
compass position.)

Reference (Firestorm, read-only): `llnetmap.cpp` (`handleDoubleClick`,
`performDoubleClickAction`, `handleRightMouseDown`, the `Minimap.*`
registrar callbacks), `menu_mini_map.xml`, `llfloaterworldmap`
(`trackLocation`), `lltracker`.

Deps: [[viewer-minimap]] (surface), [[viewer-minimap-avatar-dots]]
(dot hit-testing for the avatar context items and hover state).
