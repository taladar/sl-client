---
id: viewer-p26-1
title: Species table
topic: viewer
status: done
origin: VIEWER_ROADMAP.md — Phase 26 — Linden trees & grass
---

Context: [context/viewer.md](../context/viewer.md).

Trees and grass are classified `ObjectCategory::Other` and not rendered today.

**P26.1. Species table.** Port `app_settings/trees.xml` (the `LLVOTree`
species table) as Bevy-free data. Done: a new **`sl-tree` crate** (the
tree/grass counterpart of `sl-prim` / `sl-mesh` / `sl-sculpt`, Bevy-free and
I/O-free) holds the 21-entry table in its `species` module — one
`TreeSpecies` per species byte (diffuse `TextureKey` + every `LLVOTree`
geometry parameter: droop / twist / branches / depth / scale_step /
trunk_depth / branch/trunk length / leaf_scale / billboard scale+ratio /
trunk+branch aspect / leaf_rotate / noise / taper / repeat_z), the
`TREE_SPECIES` static, `MAX_TREE_SPECIES`, and a bounds-checked
`tree_species(byte)` lookup. Values ported verbatim from `trees.xml`; as in
Firestorm the `depth` / `trunk_depth` attributes parse as integers, so the
fractional XML values (e.g. Fern's `trunk_depth="0.1"`) truncate toward zero.
Unit-tested (index↔species_id, count, in/out-of-range lookup, texture ids,
integer truncation). P26.2 will read this table to build the geometry.
