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
is no server bake, i.e. OpenSim) currently stacks same-type clothing
layers in held wear-set order. Sort them by the COF links' ordering
tokens instead (`@type*100+index`, parsed by
`parse_order_token`) when a COF with tokens is present — so our own
render agrees with what the SL bake service (which reads the same
tokens) would produce, and with other viewers. Falls back to the
wear-set order when there is no COF or no tokens (plain OpenSim).

Touches: `bake_inputs.rs` (layer collection order); the token helpers
live in `inventory_actions.rs`.
