---
id: viewer-terrain-edit-brushes
title: Terrain editing — sculpt brushes
topic: viewer
status: done
origin: reference-viewer feature-cluster survey (2026-07); split from viewer-terrain-editing
blocked_by: [viewer-input-action-map, viewer-region-options-debug]
---

Done (2026-09-19): the Land tool is `sl-viewer-edit/src/edit_land.rs`.
`EditTool::SelectLand` joins `BUILD_TOOLS`; its panel carries the reference's
seven-way `land_radio_group` (Select Land + the six `ModifyLand` brushes), the
1 m-11 m bulldozer **Size** and **Strength** sliders (persisted as the
reference's `LandBrushSize` / `LandBrushForce` / `RadioLandBrushAction`),
**Apply to selection**, the parcel read-out, About Land / Subdivide / Join, and
the **Show owners** checkbox. A press on bare ground starts either gesture: a
brush sends one `ModifyLand` per frame at the rounded cursor point with
`Seconds = force / fps`, and **Apply** runs the reference's
`modifyLandInSelectionGlobal` scaling (0.25x level/raise/lower, 5x smooth,
0.5x noise, a fixed 0.5 s revert). `Ctrl+Z` (and Build ▸ Undo) sends
`UndoLand` instead of the object undo while a brush is picked — decided in
`edit_undo::land_undo_is_active`, the reference's `gEditMenuHandler` swap, so
the two undo stacks never eat each other's steps.
The selection, the live drag and the bulldozer footprint are drawn as
terrain-draped gizmo outlines.

Two protocol gaps had to close first. `Session::request_parcel_properties`
hard-coded `snap_selection: false`, so the flag the reply echoes meant nothing
and a click could not ask the simulator to snap to the whole parcel; it is now
a parameter on the method and on `Command::RequestParcelProperties`. And
`LandEdit.brush_size` was the three-value `LandBrushSize`, which cannot express
the reference's continuous slider -- `SimSession` decoded a 3 m brush as
`Small`. The field is now `brush_radius: LandBrushRadius`, a metre newtype the
three LSL constants convert into. `sl-repl`'s `modify_land` takes metres (or
`small` / `medium` / `large`) in that argument now rather than the old
`0`/`1`/`2` index, and rejects a radius outside the slider's travel instead of
clamping it.

**Not verified live**: the brush strokes, the drag-select and Subdivide / Join
need an estate-owner login (the local OpenSim standalone, or an owned SL
parcel). Everything client-side is unit-tested.

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
