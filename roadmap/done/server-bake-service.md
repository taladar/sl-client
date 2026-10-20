---
id: server-bake-service
title: Server-side appearance bake service
topic: server
status: done
origin: user request (2026-07) — size what a real server would involve
blocked_by: [gridspec-appearance-baking]
refs: [viewer-bake-cache-query, server-fake-grid-opensim-arrival-bakes]
---

Context: [context/server.md](../context/server.md).

SL's server-side baking: compositing an avatar's wearable textures into
the baked body-region textures on the grid (the P14-era client work
consumed this; the client also knows the pre-bake composite path), so
all observers fetch one baked set instead of every layer.

Takes the current-outfit inventory + wearable assets, composites
per-region (head/upper/lower/eyes/skirt/hair + the universal/BoM
channels), uploads results to the asset service, and answers the
appearance flow (`UpdateAvatarAppearance` cap). The client workspace
already has the compositing logic and the UV/flip gotchas documented
from the viewer side — a server implementation would share that code.
Optional for an OpenSim-style grid (client-side baking suffices there),
required for SL parity.

## From gridspec-appearance-baking (2026-10-19)

Measured in `book/src/gridspec/appearance.md` § Baking. This task finishes
what that one could not: the fake grid's Second Life flavour answers
`UpdateAvatarAppearance` with Second Life's status, `success`, `error` and
versions, but without the bake's payload, because nothing bakes. Done means:

- **Measure the payload first** (one more aditi run of `appearance-baking`):
  what an accepted answer's `textures` (45 entries), `visual_params` (253)
  and `avatar_scale` hold, and whether they are the `AvatarAppearance` that
  follows (its baked ids, its visual parameter bytes, its size); and the
  refusal's `cof_raw` (40 entries for the test avatars) and `debug_cof`
  (`avatar_scale`, `dump_joint_offsets`, `lod`, `params`, `slot_textures`
  with each slot's `hash_id` and layer `texture_ids`, `success`, `warnings`,
  `wearables`).
- **Share the client's baking, not copy it**: the layer assembly from worn
  wearables lives in the Bevy viewer (`sl-viewer-world-avatar`
  `bake_inputs.rs`) and the body size in `sl-client-bevy`
  (`body_size_metrics`); both move to pure crates the fake grid and the viewer
  share, beside `sl-bake` (plan, compositor), `sl-avatar` (wearable assets,
  visual parameters) and `sl-texture` (the baked J2C encoder).
- **Bake in the fake grid**: from the Current Outfit Folder's worn wearables
  and their assets in the grid store, composite each region, store the bakes
  under fresh ids, answer the handshake with the measured payload, and send the
  `AvatarAppearance` naming them — in place of the solid placeholder bakes.
  The per-slot `hash_id` is the reference's wearables hash
  (`LLWearableData::computeBakedTextureHash`), which
  [[viewer-bake-cache-query]] needs too.
- **Hold it**: `appearance-baking`'s `*_keys` on `FakeSl` (now only
  `*_core_keys`), plus the payload relations the measurement adds.

## Done (2026-10-20)

Measured first (`appearance-baking`, four more aditi runs): an accepted
answer's `textures` are the 45 face ids of the `AvatarAppearance` that
follows, its `visual_params` that appearance's 253 bytes as integers, and its
`avatar_scale` `[0.45, 0.6, height]`. A refusal's `cof_raw` is the Current
Outfit Folder's links plus the items they resolve to (a wearable's with its
asset text as `contents`), and `debug_cof.wearables` the worn items in
seventeen lists by wearable type. Each baked slot's `hash_id` **is** its bake's
id, and a slot no layer feeds is `IMG_DEFAULT_AVATAR`; but the id is neither
the reference viewer's wearables hash nor an MD5 of the slot's layers, so how
Second Life computes it stays unknown (`book/src/gridspec/appearance.md`
§ Baking). In one aditi run of nine the agent was sent its appearance twice
after an accepted bake; the census now holds "told", not the count.

Shared, not copied: `sl-bake` gained `outfit.rs` (`assemble_layers`,
`bake_region`, `load_static_layers`, `region_texture_ids`), moved out of
`sl-viewer-world-avatar` and `sl-viewer-kit`, which now call it; `sl-avatar`
gained `stature.rs`, the reference `computeBodySize` formula once, used by the
viewer's skeleton and by `Skeleton::body_size`; `sl-proto` gained
`avatar_texture::wearables_hash` (the reference's MD5, for
[[viewer-bake-cache-query]]) and `sim_bake.rs` (`BakedOutfit`: what a bake
answer carries, rendered into both answers by `UpdateAvatarAppearance` once
`SimSession::set_baked_outfit` names it).

The fake grid bakes (`sl-fake-grid` `bake_service.rs`): on arrival and on every
accepted bake request a `FakeSl` session resolves the worn wearables out of the
grid store, encodes their visual parameters, computes the height, and names
each slot's bake by its own MD5 of the slot, its layers and the parameters; the
appearance carries those ids. A bake is composited and J2C-encoded the first
time the appearance service is asked for it, so a test that never fetches pays
nothing; the character assets load once per process. `appearance-baking` holds
the payload on `FakeSl` (keys, outline, textures, parameters, box, the slot
rule) and passes on all four grids. A new e2e test
(`e2e_two_avatars` `the_grids_bake_reaches_the_viewer`) holds the viewer
fetching every bake the grid named.

Left: the OpenSim flavour still names the fixture's solid bakes on arrival,
where the live grid named the default placeholder for an avatar nothing has
baked — [[server-fake-grid-opensim-arrival-bakes]]. Attachments add nothing to
the fake's `lod` or joint offsets until the fake grid has worn attachments
([[fake-grid-own-attachments-and-region-moves]]).
