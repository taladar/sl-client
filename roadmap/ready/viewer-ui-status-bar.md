---
id: viewer-ui-status-bar
title: Status area (region / parcel / balance / time / FPS / permission icons)
topic: viewer
status: ready
origin: split from viewer-ui-menu-bar's status area (2026-07-20)
blocked_by: [viewer-ui-menu-bar]
refs: [viewer-ui-menu-search]
---

Context: [context/viewer.md](../context/viewer.md).

The reference viewer's **status area** (`llstatusbar` /
`panel_status_bar.xml`) — the read-outs that share the menu bar's row, to the
right of the pull-downs. Split out of [[viewer-ui-menu-bar]], which shipped the
bar and the menu names but deliberately left this for its own pass (it is a
distinct concern with its own data sources, not part of the menu mechanism).

The top menu bar is content-sized and top-**leading**, so it leaves the top
edge's trailing side free for this. Elements, in reference order (right-aligned,
hugging the trailing edge):

- **Parcel permission icons** — the current parcel's build / script / fly /
  push / voice / damage / see-avatars flags and the access state, each an icon
  shown when the permission is **denied** (`LLStatusBar::updateParcelIcons`,
  `EParcelIcon`). Parcel data comes from `ParcelProperties` over the CAPS event
  queue (see the `parcelproperties-via-caps-eventqueue` memory) — confirm the
  viewer ingests it; this may need the parcel read-model first.
- **Region name + parcel name + agent position** — the region and parcel names
  and the agent's `⟨x, y, z⟩` region-local coordinates
  (`LLAgentUI::buildLocationString`; coordinates gated on a setting).
- **L$ balance** — the agent's money balance (the `Balance`/money read-model;
  on OpenSim the balance is hardcoded 0, so aditi / SL is where it reads real).
- **Time** — the grid / region time of day.
- **FPS** — already computed for the diagnostics overlay (Bevy's
  `FrameTimeDiagnosticsPlugin`), here as a status read-out.

Most of these have a live data source already; the work is the **display** (a
content-sized, direction-neutral row of small read-outs and icons that reflows
on a font-size / locale change, per the layout conventions) plus, for the
permission icons, wiring the parcel flags. May itself split further if the
media / audio controls and the bandwidth graph are wanted.

Reference (Firestorm, read-only): `indra/newview/llstatusbar.{h,cpp}`,
`newview/skins/default/xui/en/panel_status_bar.xml`; `llpanelnearbymedia` /
`llnavigationbar` for the location controls.
