---
id: gridspec-lsl-prim-state
title: LSL library behaviour on each grid — reading and writing the prim
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, server-lsl-lib-prim-state]
blocked_by: [gridspec-lsl-live-differential-runner]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

No aditi or OpenSim measurements for this tranche beyond the library table's
documented values.

## Discover

Probes for: `llSetPos` clamp, `PRIM_*` round-trip order, which `changed` bits
each setter raises. Run on OpenSim YEngine / XEngine and aditi.

## Document

`book/src/gridspec/lsl.md` § Reading and writing the prim.

## Fake grid

Large — [[server-lsl-lib-prim-state]].

## Viewer

Prim edits made by scripts render as the grid sends them.

## Done (2026-10-27)

- **Discover.** Eight probes, `sl-lsl-runtime/probes/prim/`: `placement`
  (`llSetPos` and its 10 m cap, `llSetRegionPos`, `PRIM_POS_LOCAL` of a root,
  rotations that are not normalised, the scale clamps, the bounding box at
  once and later, `llTargetOmega`); `params` (every `PRIM_*` rule set inside
  and outside its ranges and read back, the newer rules by number, a fresh
  prim's values); `params-rules` (order, `ALL_SIDES`, faces out of range,
  unknown codes, wrong types, too few values, `PRIM_LINK_TARGET` and the link
  functions on a single prim); `faces` (the per-face setters and getters, hover
  text, the faces of each shape); `identity` (name and description limits and
  characters, the click action, `llGetObjectDetails` code by code); `link`
  (linking with and without permission, the link numbers, the `LINK_*`
  constants, moving a child, breaking links); `changed` (which setter raises
  which bit); `effects` (the sound, particle, texture-animation and media
  calls' errors). Complete on aditi and both OpenSim engines.
- **Found on the way.** YEngine throws on a get of a face it cannot index and
  on a particle rule with no value; both are in scripts of their own.
  `PRIM_CAST_SHADOWS` set never returns under XEngine, so that script is left
  out there. Second Life returns a prim moved by `llSetRegionPos` past an edge
  with no neighbour to its owner's inventory, so the probe stays inside the
  region. All in `book/src/tools/lsl-probes.md`.
- **Document.** `book/src/gridspec/lsl.md` § Reading and writing the prim,
  linked from `content/scripts.md`.
- **Fake grid.** Large, as planned: the measured behaviour per flavour is in
  [[server-lsl-lib-prim-state]].
- **Viewer.** None: every setter ends in an object update the viewer already
  renders as sent.
