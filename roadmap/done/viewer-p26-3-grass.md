---
id: viewer-p26-3
title: Grass
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 26 — Linden trees & grass
---

Context: [context/viewer.md](../context/viewer.md).

**P26.3. Grass.** Render pcode-grass as the reference
crossed-quad patches (`LLVOGrass`) with the species texture. Done: `sl-tree`
grew a Bevy-free `grass` module porting `LLVOGrass::getGeometry` /
`LLVOGrass::initClass` — a fan of up to `GRASS_MAX_BLADES` (32) leaning
two-sided blade *cards* (8 vertices / 12 indices each, front and back copies
with opposite normals) scattered around the object centre with a Gaussian
spread, into one `GrassMesh` — plus a `grass` species table (`GrassSpecies` /
`GRASS_SPECIES`, 6 entries) ported from `app_settings/grass.xml` (diffuse
texture + `blade_size_x` / `blade_size_y`), with a `grass_species` lookup. The
reference multiplies the blade-centre scatter by the object scale (`x =
exp_x * mScale`) but sizes each card from the species table, so the object
scale is folded into the *spread* inside the generated geometry (absolute
metres), **not** applied as a mesh scale — the winding, the leaning `- xf`
base-2 quirk, the forced `+0.75` blade-normal Z, and the `u`/`v` card UVs are
ported verbatim, unit-tested for counts / clamping / scale-spread.
`sl-client-bevy` adds `to_bevy_grass_mesh` and re-exports the grass API; the
viewer gains an `ObjectCategory::Grass` (classified from `PCODE_GRASS`) and
builds one face entity textured with the species diffuse (a synthetic white
`TextureFace` through the Phase-6 pipeline, `AlphaMode::Blend` to match the
reference's `PASS_GRASS` / `POOL_ALPHA` soft-edged blades), placed by an
**identity** geometry-holder transform (the object scale already lives in the
mesh spread). Since a grass clump's geometry depends on the object scale —
where a prim's / tree's does not — the object's X/Y scale is folded into a
grass-only field of the geometry-rebuild `ShapeFingerprint` (quantised to
mm), so a live resize rebuilds the clump while never re-tessellating any
other category. Verified live on OpenSim (a new `rez_sample_grass` example
rezzes a row of all six species): the blade fans render as upright wispy
grass with the species texture. Three faithful simplifications, documented in
the module: blade bases sit on the object's local `z = 0` plane rather than
each sampling the terrain height (`resolveHeightRegion`, needs a heightfield
this I/O-free crate lacks); the per-blade scatter comes from a fixed-seed PRNG
reproducing the reference's Box–Muller distribution (the reference seeds
`ll_frand` from a *random* UUID, so its exact layout differs every run and is
shared by all grass — we likewise share one stable layout); and wind sway is
not simulated. No blade-count distance LOD (the reference sheds blades for
performance; not required here).
