---
id: viewer-parcel-owners-terrain-overlay
title: Land Owners — in-world terrain ownership colour fill
topic: viewer
status: ready
origin: Firestorm full-parity audit (2026-08-19)
refs: [viewer-parcel-overlay-decode, viewer-parcel-grid-flood-fill,
  viewer-terrain-edit-brushes, viewer-parcel-borders-render]
---

Context: [context/viewer.md](../context/viewer.md).

World ▸ Show More ▸ Land Owners (`ShowParcelOwners`, menu_viewer.xml
L1243) tints the terrain itself by parcel-ownership class — self /
group / other owner / for-sale / auction — the in-world equivalent of
the minimap's parcel colouring. The reference draws it in the terrain
draw pool (`lldrawpoolterrain.cpp`) whenever the toggle or the Land
tool's "Show owners" checkbox (`llpanelland.cpp`) is on.

We colour the parcel border *lines* by ownership
(`sl-client-bevy-viewer/src/parcel_borders.rs`,
[[viewer-parcel-borders-render]] done) and fill the minimap
(`sl-client-bevy-viewer/src/minimap.rs`), both fed from the decoded
ParcelOverlay grid ([[viewer-parcel-overlay-decode]] done), but have
no in-world terrain fill. Scope: a terrain overlay pass fed from the
overlay grid's ownership classes, the World ▸ Show More menu toggle,
and the Land-tool checkbox once the land tool lands
([[viewer-terrain-edit-brushes]]).

Reference (Firestorm, read-only): `indra/newview/lldrawpoolterrain.cpp`
(ShowParcelOwners overlay texture), `indra/newview/llpanelland.cpp`,
`indra/newview/skins/default/xui/en/menu_viewer.xml` L1243.
