---
id: viewer-tree-species-all-rendered-as-trees
title: All tree/plant objects render as the same (large) tree — species not mapped (ferns/small plants shown as trees)
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

SL encodes the plant species in the object's `state` byte (`LLVOTree`'s
`tree_species`, ~30 species incl. several small ground plants / ferns /
grasses). Our procedural tree build is `build_tree_faces(object.state, …)`
(`ObjectCategory::Tree`), backed by the P26.1 species table
([[viewer-p26-1]]).

## Likely cause

- The **species table is incomplete / defaults unknown or small-plant species to
  one big-tree model** — so ferns / grasses / bushes render as full trees at
  full tree size. Check `build_tree_faces` and the P26.1 table: does every SL
  `tree_species` map to a distinct plant model + size, and what does an unmapped
  species fall back to?
- Note `ObjectCategory::Grass` (`build_grass_faces`) is a *separate* path; some
  ground plants come as `Grass`, others as small-`Tree` species — confirm which
  category each misrendered plant arrives as.

## Next

Log the `state`/species byte of the misrendered plants and compare against the
P26.1 species table + the reference `LLVOTree` species list; add / correct the
small-plant / fern / grass species so they render at the right form and scale
(or as the reference's billboard imposter) instead of a full tree.
