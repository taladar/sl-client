---
id: viewer-terrain-edit-brushes
title: Terrain editing — sculpt brushes
topic: viewer
status: ready
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-terrain-editing
blocked_by: [viewer-input-action-map, viewer-region-options-debug]
---

Context: [context/viewer.md](../context/viewer.md).

In-world terrain sculpting brushes: raise / lower / flatten / smooth / roughen /
revert over a selected land area. The brush drag uses input **actions**
([[viewer-input-action-map]]) and sends the `ModifyLand` message; brush size /
strength selection lives next to the region floater
([[viewer-region-options-debug]]), which owns the terrain-limit and
terrain-texture controls the editing overlaps with.

Reference (Firestorm, read-only): `lltoolbrushland` (`LLToolBrushLand`); the
`ModifyLand` message.

Builds on: `terrain.rs` and `sl-terrain`.

Deps: [[viewer-input-action-map]] (brush drag),
[[viewer-region-options-debug]] (terrain textures / heights overlap).

## Parity-audit addendum (2026-08-19)

The build floater's Land panel integration goes beyond the brush
radios already in the body: the **Select Land** rectangle mode (`radio
select land` — drag a land rectangle as the operand), the **Apply to
selection** button (run the chosen brush over the selected land rect
instead of under the cursor), and the **ShowParcelOwners** checkbox
(the ownership-colour ground overlay toggled from the Land panel — the
overlay itself is [[viewer-parcel-owners-terrain-overlay]]).
References: `floater_tools.xml` L682-816 and 3427. The panel's parcel
buttons row (About Land / Subdivide / Join / Buy / Abandon) is covered
elsewhere (about-land done, viewer-parcel-join-split,
viewer-money-economy-ui).
