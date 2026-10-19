---
id: server-bake-service
title: Server-side appearance bake service
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
blocked_by: [gridspec-appearance-baking]
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
