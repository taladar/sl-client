---
id: viewer-ui-status-bar
title: Status area (region / parcel / balance / time / FPS / permission icons)
topic: viewer
status: done
origin: split from viewer-ui-menu-bar's status area (2026-07-20)
blocked_by: [viewer-ui-menu-bar]
refs: [viewer-ui-menu-search]
---

**Done (2026-07-20).** `src/status_bar.rs` (+ `status_bar/slt.rs`). The status
read-outs now fill the top menu bar's row: `spawn_status_area` hangs them off
the (now full-width) menu bar after the search field as one flex item, so the
top row reads as a single continuous bar spanning the window. In reference
order: the parcel-permission placeholders, the region name, the coordinates, the
**flexible** parcel name (absorbs the row's slack, pushing the rest right), then
the balance, time and FPS. Every element but the parcel name is fixed-width, so
a changing value's text length never jitters its neighbours. All numbers /
currency / timestamps go through the locale-aware `Translator`.

- **Time is always SLT** (US Pacific, DST-correct), computed in `slt.rs` from
  the UTC instant with the US daylight-saving rules — no time-zone database.
- **Balance** held as `LindenBalance`, requested on region entry (OpenSim
  reports a hardcoded 0; aditi/SL reads real).
- **Coordinates** gated on a new `[statusbar] show_coordinates` setting (default
  on), mirroring the reference `NavBarShowCoordinates`.
- **Permission logic** mirrors `LLViewerParcelMgr::allowAgent*` for voice / fly
  / push / build / scripts / see-avatars / damage, from `SlAgentParcel` + the
  region `RegionFlags`. New flag/helper support added: `sl-wire` `ParcelFlags`
  `ALLOW_VOICE` / `USE_ESTATE_VOICE_CHAN`, `RegionFlags` `RESTRICT_PUSHOBJECT` /
  `ESTATE_SKIP_SCRIPTS`; `sl-proto`
  `ParcelInfo::{allow_voice, allow_other_scripts, restrict_push, allow_damage}`.
- **FPS** moved here from the debug diagnostics overlay; that overlay
  (frame-time ms + the entity/draw line) was removed entirely, leaving only the
  F3 pipeline-status panel in `src/diagnostics.rs`.

Deviations / interim choices (chosen with the user during the live review):

- Permission indicators are **single-letter text placeholders**
  (`V F P B S A D`, brightening when in force), not the reference's parcel-icon
  textures — the viewer bundles no icon art. Porting the real icons is a
  **follow-up task**.
- The voice placeholder uses only the parcel `ALLOW_VOICE` flag (no region
  "voice enabled" signal exists), so it is a close, not bit-exact, reproduction.
- Omitted (per the roadmap's own "may split further" note, or lacking data): the
  media / audio controls, the bandwidth graph, the pathfinding-dirty/-disabled
  icons (SL navmesh state not tracked) and the damage-% text.

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
