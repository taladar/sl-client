---
id: gridspec-seed-capabilities
title: Which capabilities each grid grants from the seed, and to whom
topic: gridspec
status: ready
origin: user question during gridspec-login (2026-10-04)
refs: [gridspec-survey, gridspec-simulator-features, gridspec-region-arrival,
  gridspec-neighbours-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

The survey's tasks measure what individual capabilities *answer*
([[gridspec-simulator-features]], [[gridspec-inventory-fetch]], the asset
fetch and upload tasks, …) but none measures the **capability set** itself:
which names a region's seed capability grants on each grid. That set decides
which code path a viewer takes for almost everything, and it is where the two
grids differ most visibly.

## Known already

The client asks for `REQUESTED_CAPABILITIES` (`sl-proto`'s `session.rs`); the
fake grid grants `SERVED_CAPABILITIES` (`sl-proto`'s `sim_caps.rs`, a
`CapStatus` coverage table beside it), the same for both flavours apart from
the bake trigger (`UpdateAvatarAppearance`, Second Life only) and whatever a
test's region config withholds. What aditi and
OpenSim grant has never been recorded as a set: the cases only check the
capability they need is there, or mark themselves partial when it is not.

## Discover

- A `seed-capabilities` conformance case (or extend `login-handshake`):
  record the names the seed granted against the names requested, on the root
  region, and on a child region where the grid opens one
  ([[gridspec-neighbours-crossing]]); both grids, one avatar.
- Ask for the reference viewer's full list (Firestorm's
  `LLViewerRegion::buildCapabilityNames`) as well as ours, so a capability we
  do not request yet but a grid offers is visible.
- Note which granted capabilities answer at all (a granted name whose URL
  404s is a finding), without exercising each — that is the per-feature tasks'
  job.

## Document

`book/src/gridspec/capabilities.md`: the granted set per grid (requested by
us, requested by the reference, granted), root and child; link from
`comms/caps.md`.

## Fake grid

Small, in this task: per-flavour granted sets in `imitates.rs` (a capability
the live grid does not grant is not granted by its flavour either), held by
the case as a `Measured` set on both fake flavours. Implementing a capability
the fake grid lacks entirely belongs to the per-feature task.

## Viewer

Every capability the viewer requests and a grid does not grant needs a
fallback or a refusal the user can see; list them, check each, file the gaps.
