# sl-tree

Linden **tree & grass** data and geometry for Second Life / OpenSim clients —
the `LLVOTree` / `LLVOGrass` counterpart of `sl-prim` (parametric prims),
`sl-mesh` (LLMesh) and `sl-sculpt` (sculpt maps).

Trees and grass are not ordinary prims: they are their own object classes
(`PCODE_TREE` / `PCODE_NEW_TREE` / `PCODE_GRASS`) whose visible form is driven
entirely by a one-byte *species* selector carried in the object's `state`
field. The species indexes a fixed table — Linden's `app_settings/trees.xml` —
that gives each species its diffuse texture and the parameters of its
procedurally generated geometry (branch length, droop, taper, billboard scale,
…).

This crate ports that species table as **Bevy-free, I/O-free data**, so the
geometry can be generated once and the Bevy conversion kept in
`sl-client-bevy`, exactly like its sibling geometry crates.

Currently implemented:

- [`species`] — the 21-entry `LLVOTree` species table
  ([`TreeSpecies`] / [`TREE_SPECIES`]), ported verbatim from `trees.xml`, with
  a [`tree_species`] lookup by species byte. Each entry carries the species
  diffuse [`TextureKey`](sl_types::key::TextureKey) plus the `LLVOTree`
  geometry parameters.
- [`geometry`] — the procedural `LLVOTree` branch / leaf geometry generation
  ([`tree_geometry`] / [`billboard_geometry`]) ported from
  `LLVOTree::updateGeometry` / `genBranchPipeline`, producing a Bevy-free
  [`TreeMesh`] at one of four [`TreeLod`] trunk levels or as a distance
  billboard imposter.

The grass geometry (crossed quads) and the grass species table land in later
phases.
