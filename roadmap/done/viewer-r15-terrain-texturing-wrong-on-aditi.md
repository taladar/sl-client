---
id: viewer-r15
title: Terrain texturing wrong on Aditi
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Known rendering issues (to fix)
---

Context: [context/viewer.md](../context/viewer.md).

**R15. Terrain texturing wrong on Aditi** (`sl-proto` / `sl-client-bevy`).
Root cause found (new `terrain-composition` conformance case, live on both
grids): a modern Second Life mainland region leaves its four
`TerrainDetail` ids **nil** in the `RegionHandshake` and drives the ground
appearance another way, so the splat had nothing to fetch and rendered
flat. This is *not* a parse bug — the case confirmed the `RegionInfo`
fields that sit after the terrain block (`RegionID` / `ProductName` /
`ProductSKU`) and the elevation bands all parse correctly while the ids
are nil (aditi region "Mauve": `product_name = "Mainland / Full Region"`,
`start_height 20` / `range 60`, all four detail ids nil). The reference
viewer keeps rendering here because `LLVLComposition::setDetailAssetID`
early-returns on a nil id, leaving the four
**default Linden terrain textures** (dirt / grass / mountain / rock) its
composition was seeded with. Fix: a new
`RegionTerrainComposition::detail_textures_or_default()` substitutes those
defaults (`DEFAULT_TERRAIN_DETAIL_TEXTURES`, in `sl-proto`) for nil slots,
and the viewer requests the effective ids — the case shows all four
defaults fetch and decode over `GetTexture` on aditi
(`terrain_mode = "default-substituted"`, complete). A **second** bug
(found by a live viewer run against aditi) stacked on top: the terrain
composition is learned during the `RegionHandshake`, *before* the seed
capabilities arrive, so the boosted `GetTexture` fetch failed permanently
("capability not available") and the ground stayed flat even with the
defaults. Fix: the texture / mesh / wearable / animation managers now
**hold** a request whose capability is not set yet and re-issue it once
the cap arrives (`retry_pending*`), rather than fail it — a general
latent-race guard (terrain is the only consumer that requests before caps,
so it was the only one that reliably triggered it). Verified end to end by
a windowed run: the aditi mainland ground renders the default dirt / grass
/ mountain / rock splat, matching Firestorm. Still deferred to
**Phase 27**: a region that sends *non-nil* GLTF **material** ids (PBR
terrain) — those do not decode as J2C, so the case marks that partial.
Candidate cause (1), fetch-queue starvation, was already addressed by the
Phase 20 `BOOST_TERRAIN` priority. Reference:
`LLVLComposition::setDetailAssetID` / `getDefaultTextures`,
`indra_constants.h` `TERRAIN_*_DETAIL`.
