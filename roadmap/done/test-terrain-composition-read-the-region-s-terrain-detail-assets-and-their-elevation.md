---
id: test-terrain-composition
title: read the region's terrain detail assets and their elevation bands, and
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 11 — Region, estate & map `[both]`
---

Context: [context/test.md](../context/test.md).

`terrain-composition` — read the region's terrain detail assets and their
    elevation bands, and probe legacy vs PBR-material terrain. `1av`.
    **Complete on both grids.** Waits for [`Event::RegionInfoHandshake`]
    *directly* (it is pushed immediately before `RegionHandshakeComplete` on
    the root circuit, so a prior `wait_for_region` would consume and discard
    it), then records the four `TerrainDetail` ids, the per-corner
    start-height/height-range bands, and the `RegionInfo` fields that sit
    **after** the terrain block (`RegionID` / `ProductName` / `ProductSKU`) —
    the last of these prove the block parsed at the right offsets. It then
    fetches+decodes each *effective* detail id (the region's own id, or the
    `DEFAULT_TERRAIN_DETAIL_TEXTURES` fallback for a nil slot) over
    `GetTexture` through the shared `TextureStore`. Underpins viewer R15:
    OpenSim sends four real J2C ids (`terrain_mode = "legacy-texture"`); the
    aditi mainland region "Mauve" sends **all four nil** but everything else
    parses (product `Mainland / Full Region`, `start_height 20` / `range 60`),
    so the defaults are substituted and all four decode
    (`terrain_mode = "default-substituted"`). A region that sent *non-nil*
    GLTF material ids would decode none and mark the run partial (PBR terrain
    — Phase 27). Records the ids, per-slot decoded/substituted flags, the
    declared/substituted/decoded counts, and the mode.
