---
id: viewer-bake-cof-layer-order
title: Client-side bake honours COF layer-ordering tokens
topic: viewer
status: ready
origin: user request (2026-07-22); consistency follow-up to the COF
  layer-ordering tokens
refs: [viewer-inventory-cof-maintenance, viewer-outfit-layer-reorder]
---

Context: [context/viewer.md](../context/viewer.md).

The **client-side compositor** (the P15.3 composite used where there
is no server bake, i.e. OpenSim, and for the appearance editor's preview
everywhere) draws every worn wearable of a type, in the order the bake
inputs hold them — bottom first (`sl-bake` `WornLayer`, since
2026-11-04; before that it drew only the first of each type, which is
why this ordering never showed).

Where that order comes from:

- **Second Life** answers `AgentWearablesRequest` with placeholders, so
  the outfit is read from the Current Outfit Folder, already sorted by
  the links' ordering tokens (`@type*100+index`, `cof_wearables` in
  `bake_inputs.rs`). Done.
- **OpenSim** answers with the outfit it holds, in its own wear-set
  order, and the COF is not consulted. **This is what is left:** when a
  COF with tokens is present there, order the held wear set by them —
  so our own render agrees with what the SL bake service (which reads
  the same tokens) would produce, and with other viewers. Fall back to
  the wear-set order when there is no COF or no tokens (plain OpenSim).

Touches: `bake_inputs.rs` (outfit order: `take_outfit` / `CofOutfit`).
