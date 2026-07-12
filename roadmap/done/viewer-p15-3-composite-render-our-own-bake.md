---
id: viewer-p15-3
title: Composite & render our own bake
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 15 — Client-side baking (`sl-bake`, the OpenSim/legacy path)
---

Context: [context/viewer.md](../context/viewer.md).

**P15.3. Composite & render our own bake.** When no server bake is
published for an avatar (our own on OpenSim), composite its regions with
`sl-bake` and drive the Phase-14 body-region materials + Phase-17 BoM from the
local composite instead of a fetched baked UUID (alpha honoured). Verify our
own avatar renders skin/clothing-textured on OpenSim. **Done (Phase-14 body
regions; the Phase-17 BoM half is deferred with Phase 17):** a new
`OwnLocalBake` resource + `apply_own_local_bake` system (`avatars.rs`)
composites each ready `OwnBakeInputs` region (P15.2) through
`composite_region` at 512², uploads it, and drapes it onto our own avatar's
body-region materials for every slot the grid did **not** server-bake —
reusing the P14 per-`(agent, slot)` region material so a real server bake
(Second Life) still wins, and self-healing after
`assign_avatar_bake_materials` resets a part. A region with no worn layers is
skipped (an empty composite is fully transparent and would wrongly carve the
region). Two live-found orientation/alpha fixes were needed on top of the
plan: (a) Second Life avatar `.llm` UVs are OpenGL bottom-up, so the
composited bake (top-down, like every decoded J2C) is flipped vertically
before upload (`flip_rows_vertically`), else the head bake reads
upside down (chin/teeth on the forehead); (b) the eyeball is opaque geometry
but our simplified eye composite carries only the iris layer (not the opaque
sclera base the reference eye layer-set builds), whose transparent surround
classified the bake `Masked` and carved the eyeballs into empty sockets — so
the eyes region bake is forced opaque (`force_alpha_opaque`). Verified live on
OpenSim: our own avatar renders skin/clothing-textured, right-way-up, with
visible eyeballs (default outfit composites `head`/`upper`/`lower` opaque +
`eyes` forced-opaque + `hair` masked; `skirt` empty). The eyeball vertical
placement issue this surfaced is tracked separately as P15.5.
