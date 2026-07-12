---
id: viewer-p14-2
title: Map bakes onto body regions
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 14 — Server-published baked texturing (incl. alpha)
---

Context: [context/viewer.md](../context/viewer.md).

**P14.2. Map bakes onto body regions.** Build one `StandardMaterial` per
base-body region from its baked slot (head→head, upper→upper body, lower→lower
body, eyes→eyes, hair→hair, skirt→skirt), uploaded via the same
`TextureDecoded` path as `apply_prim_textures`. Verify a textured other-avatar
body on both grids.

**Done (P14.1 + P14.2 bundled).** `ingest_avatar_bakes` reads the six
base-body baked slots (`BODY_BAKE_SLOTS`) from each `AvatarAppearance`'s
`texture_entry`, keeps only the visible bakes
(`avatar_texture::is_bake_visible`) via `visible_body_bakes`, requests each
through the shared `TextureManager`, and tracks them per avatar in
`AvatarState::baked_textures`. `assign_avatar_bake_materials` gives every base
part a per-`(avatar, region)` `StandardMaterial` (new `AvatarBakeMaterials`
resource) — deferred/idempotent like `apply_avatar_appearance` (dirty set +
`Added<AvatarBodyPart>`), a region with no bake keeping the shared skin
material; `apply_avatar_bake_textures` fills each material's
`base_color_texture` (and resets `base_color` to white so the composited bake
is untinted) as the bake decodes, mirroring `apply_prim_textures`. A body-part
material query pushed the `Update` tuple past Bevy's 20-system cap, so the
appearance/bake systems are nested into one sub-tuple.

**Own-avatar bake trigger (net-new, beyond the listed items).** The viewer is
a passive renderer, so on a central-baking grid our *own* avatar was never
baked → an untextured cloud → nothing for P14 to fetch. New `appearance.rs`
(`ServerBakeState` + `drive_server_bake`) drives the modern SL server-side
bake: on seeing the `UpdateAvatarAppearance` cap it reads the current Current
Outfit Folder version from the login-seeded inventory skeleton
(`Command::QueryInventoryFolders` → `Event::InventoryFolders`, the same model
the inventory cache is built on — `current_outfit_version`) and POSTs
`RequestServerAppearanceUpdate { cof_version }`, retrying with the grid's
`expected` version on a mismatch (bounded). Net-new library surface: a public
`pub use sl_proto::CAP_UPDATE_AVATAR_APPEARANCE` re-export from
`sl-client-bevy` (matching `CAP_GET_TEXTURE`). This is the
`server-appearance-bake` conformance handshake, now driven from the viewer.

**Verified live on aditi (SL):** the trigger read COF version 15, the grid
accepted the bake in one attempt, our own `AvatarAppearance` then arrived with
5 real bakes, and the body-region materials were assigned to 7 parts — the
avatar body renders textured (user-confirmed on screen). Inert on OpenSim
(no `UpdateAvatarAppearance` cap; our own OpenSim bake is the Phase-15
client-bake gap).

**`sl-texture` decoder fix (net-new, fell out of live verification).** Only
*part* of the body (and some prims/terrain) textured at first: the store's
full-resolution fetch stopped at the viewer's `1/8`-rate byte *estimate*
(`Header::discard_data_size(0)` / `calcDataSizeJ2C`), which for a texture that
compresses worse than 8:1 truncates the codestream mid-tile-part, so OpenJPEG
rejects it (`jpeg2k` "Tile part length size inconsistent with stream length").
The estimate is only a valid prefix boundary for *coarser* LODs. Fix:
`TextureStore::upgrade` now decodes the fast estimate prefix first (unchanged
for the well-compressing majority) and, only when that decode *fails* and the
codestream is not yet complete, grows to a new `Header::full_data_size_bound`
(the uncompressed-size upper bound — always enough) and decodes once more. So
the rare failing texture recovers without slowing the common path (a first
attempt to always-fetch-full made *every* texture pull ~8× the bytes and
crawled — reverted). Verified live on aditi: 299 texture decodes in 90 s (was
~52 under the always-full attempt), the single truncating texture recovered by
retry, avatar + scene textured. This is a shared `sl-texture` / `sl-proto`
change benefiting all textures, not just avatar bakes.
