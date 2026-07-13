---
id: protocol-66
title: Session tracks the agent's current parcel & fly permission
topic: protocol
status: done
origin: follow-up to viewer-p31-11 (auto-stop flying) — the takeoff half needs a
  reusable fly-permission source
---

Context: [context/protocol.md](../context/protocol.md).

The auto-stop-flying-on-landing work (viewer-p31-11) revealed the viewer has no
notion of the agent's **current parcel** or whether flying is permitted. Rather
than have every consumer (bevy viewer, tokio client) re-derive it, fold it into
the shared `sl-proto` `Session` as read-model state alongside the existing
folded caches (friends / online / chat sessions).

Scope:

- Fold the current region's `RegionFlags` (from `RegionHandshake`) into a
  per-circuit cache; expose `Session::region_blocks_fly` (the
  `REGION_FLAGS_BLOCK_FLY` bit, `1 << 19`). Add the `BLOCK_FLY` /
  `BLOCK_FLYOVER` constants to `sl_wire::RegionFlags`.
- Fold `ParcelProperties` (UDP + CAPS event-queue paths) into a per-circuit
  parcel cache. The simulator auto-pushes the agent's parcel with
  `SequenceID == 0` on region entry / parcel crossing (OpenSim
  `LandManagementModule.EventManagerOnClientMovement`), so passive folding is
  enough; drop the caches with the rest of a retiring circuit's state.
- Resolve the agent's **current parcel** from the own-avatar object position and
  each parcel's membership `bitmap` (one bit per 4×4 m block, index
  `x/4 + (y/4) * edge`, `edge = isqrt(bitmap_bits)` so var-size regions work):
  `Session::current_parcel`.
- `Session::can_fly`: mirrors Firestorm `LLAgent::canFly` — `false` if the
  region blocks fly, else the current parcel's `ALLOW_FLY`. Permissive when the
  parcel is not yet resolved (don't block a takeoff before the push arrives),
  which differs from the reference's deny-if-no-parcel (the reference always has
  the agent parcel). Add `ParcelInfo::allow_fly`.
- Bridge the result into `sl-client-bevy` as a resource
  (`SlAgentParcel { current, can_fly }`) updated from the driven session each
  frame, so ECS systems can read it. `sl-client-tokio` calls the accessors
  directly.

Unit-test the bitmap resolution (block indexing, edge derivation, multi-parcel
region) and the `can_fly` combination (region block-fly, parcel allow/deny,
unknown-parcel permissive).
