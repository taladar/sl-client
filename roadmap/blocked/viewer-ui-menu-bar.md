---
id: viewer-ui-menu-bar
title: Top menu bar
topic: viewer
status: blocked
origin: gap noticed reviewing the UI cluster (2026-07)
blocked_by: [viewer-ui-context-menu]
refs: [viewer-ui-bottom-toolbar]
---

Context: [context/viewer.md](../context/viewer.md).

The viewer's **top menu bar** — the horizontal strip of pull-down menus
(Avatar / Comm / World / Build / Content, or Firestorm's Me / Comm / World /
Build / Content / Help arrangement) — each opening a line-based menu of actions
and toggles. Builds on the line-based menu mechanism
([[viewer-ui-context-menu]]); this task is the bar itself, the menu tree, and
wiring each entry to its command / floater toggle.

Per the layout conventions the bar and its menus size to content and reflow on a
font-size / locale change.

Reference (Firestorm, read-only): `llmenubarview`, `menu_viewer.xml`.

## Status area (2026-07-18)

The top bar is more than the pull-downs — it also carries the reference viewer's
**status area** (`llstatusbar`), to the right of / alongside the menus:

- **Menu search** — a search field that finds an action within the menus.
- **Region / parcel permission icons** — the current parcel's build / script /
  fly / push / voice / damage / see-avatars flags, and the region/parcel access
  state, shown as icons (parcel data comes from `ParcelProperties` over the CAPS
  event queue).
- **Region name + position** — the current region name and the agent's
  `⟨x, y, z⟩` region-local coordinates.
- **Parcel name** — the current parcel's name.
- **Money balance** — the agent's L$ balance.
- **Time** — the grid / region time of day.
- **FPS** — the frame rate (already available from Bevy's
  `FrameTimeDiagnosticsPlugin`).

Most of these have a live data source already (agent position, region handle,
parcel properties, balance, FPS); this is the display. May split into its own
`viewer-ui-status-bar` widget if it outgrows the menu-bar task.

Reference (Firestorm, read-only): `llstatusbar`, `llpanelnearbymedia` /
`llnavigationbar` for the location controls.
