---
id: test-modify-land
title: raise/lower terrain, then undo
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 10 — Parcel & land `[both]`
---

Context: [context/test.md](../context/test.md).

`modify-land` — raise/lower terrain, then undo. `1av`. Runs as the
estate-owner avatar (`--avatar estate-owner`), who owns the region-wide parcel
and so has terraform rights; on OpenSim it forces a login at the region centre
so the avatar is within terrain-streaming range of the edited patch. A
terraform edit is a `ModifyLand` ([`Command::ModifyLand`]) brush stroke — a
[`LandEdit`] bundling a [`LandBrushAction`], a [`LandBrushSize`], a strength,
and the region-local ground rectangle ([`TerraformArea`]); the viewer sends a
zero-area rectangle at the cursor ([`TerraformArea::point`]) for a click-drag
brush, whose cos-falloff sphere moves the very centre cell by the full
strength. There is no reply for a terraform edit — the confirmation is the
simulator re-broadcasting the affected `LayerData` terrain patch
([`Event::TerrainPatch`]) with the new heights; the region centre (128, 128)
is patch (8, 8) cell (0, 0), exactly the brush peak. Flow: learn the
region-centre parcel's local id (and confirm we own it) from a
`ParcelPropertiesRequest`; advertise a `Throttle` so the sim streams terrain
and drain the login terrain flood; raise the centre and read the raised height
`H1` off the re-broadcast patch; send `UndoLand` ([`Command::UndoLand`]) and
watch for the patch to drop back to the baseline `H0`; assert `H1 - H0` is a
real rise. New client code: only the `LandEdit`/`LandBrushAction`/
`LandBrushSize`/`TerraformArea` re-exports from `sl-client-tokio` (the
`ModifyLand`/`UndoLand` command surface and terrain-patch decode all already
existed) — same re-export gap as `d41e378`. **Green on OpenSim's Default
Region as partial:** the raise is verified (baseline 24.95 m → 28.02 m, delta
3.06 m for a 3 m brush; DCT quantisation adds the extra), but stock OpenSim's
`UndoLand` is a **no-op** (the terrain module's `client_OnLandUndo` is an
empty stub), so the undo re-broadcasts nothing and the wait times out; the
case restores the terrain with a `Revert` brush (reverts toward the baked
heightmap) and reads back the baseline, and marks the run **partial** — undo
restoration is only assertable on a grid that honours `UndoLand`. Either way
the region is left as found. `[both]`; the aditi run (which can assert the
undo restore) is deferred with the batch.
