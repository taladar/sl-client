---
id: viewer-tree-species-all-rendered-as-trees
title: All SL trees render as one large evergreen — species read from `state` instead of the `Data` genome (fix landed, needs live verify)
topic: viewer
status: bugs
origin: noticed live on aditi comparing our render to Firestorm (2026-08-11)
---

Context: [context/viewer.md](../context/viewer.md).

Live on aditi vs the Firestorm reference render of the same region: we appear to
render **more trees, some in strange places** — but the real cause (per the
user) is that we render **every `Tree`/plant object as the same large tree**,
whereas the reference renders the actual **species**: ferns, grasses, bushes,
small trees, palms, etc. A small-plant species (e.g. a fern) drawn as a big tree
reads as "an extra tree in a strange place", which is what the count/placement
mismatch actually is.

The user confirmed the shape: big **evergreens** (species 0, "Pine 1") where the
reference has small **autumn-coloured trees and ferns**. That uniform-evergreen
signature is species byte `0`, i.e. the species was being read from the wrong
byte and defaulting to `0`.

## ROOT CAUSE (2026-08-11): tree species read from `state`, not the `Data` genome

The P26.1 species table is complete and correct (21 species incl. Fern, indexed
by species byte), and the geometry (`sl_tree`) is species-driven — so neither
was the problem. The bug was **which byte we read as the species**.

`build_tree_faces` (`ObjectCategory::Tree`) was passed **`object.state`**. But
the reference viewer reads the tree species from the object's
**`Data` (genome)** field, not `State`: `LLVOTree::processUpdateMessage` →
`mSpecies = ((U8 *)mData)[0]` (`indra/newview/llvotree.cpp`). Second Life leaves
a tree's `State` at `0` and carries the species only in `Data` (a full update's
one-byte `Data` field; a compressed update's inline genome byte under the tree
flag — `object_update/compressed.rs` already decodes it into `object.data`). So
on SL every tree read `state == 0` → species 0 = "Pine 1", a large evergreen.

OpenSim, by contrast, packs the species into **both** `State` and `Data`
(`LLClientView.CreatePrimUpdateBlock`, `AddByte(state)` for the header *and* for
the `Data` field), which is why trees looked right on the local grid and the bug
only showed on aditi. Grass is unaffected — the reference reads *grass* species
from `State` (`LLVOGrass::updateSpecies` → `getAttachmentState`), so
`build_grass_faces(object.state)` stays correct.

## FIX LANDED (2026-08-11)

New `tree_species_byte(object)` = `object.data.first().unwrap_or(0)` — the
`Data` genome (correct on SL and OpenSim), defaulting to species `0` for a
degenerate update with no `Data`, exactly as the reference does. It deliberately
does **not** fall back to `state`: the reference never reads `State` for a tree
species, both grids always send `Data`, and a `state` fallback would only
reintroduce this bug (`state` is `0` on SL) or pick garbage. Used at both the
initial build and the `PendingTree` LOD-rebuild inputs. Unit test
`tree_species_reads_the_data_genome_not_state`.

**Needs live verify on aditi:** confirm the region now shows varied species
(ferns / autumn trees) instead of uniform evergreens.

Related: [[viewer-p26-1]] (the species table this fix feeds correctly at last).
