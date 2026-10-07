---
id: gridspec-simulator-features
title: The full SimulatorFeatures map on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, viewer-opensim-region-extras-limits,
  server-fake-grid-lsl-syntax, viewer-simulator-features-unread-keys]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-07)

Measured and written up in `book/src/gridspec/region-arrival.md`
§ SimulatorFeatures.

- **Discover.** `simulator-features` now records every key of the reply
  (nested ones by dotted path, as `feature.<path>` metrics) and holds each
  grid to the keys it was measured sending and the LLSD kind of each. It ran
  on aditi from two regions (the sandbox and a mainland region, on two
  simulator versions) and on OpenSim's `Default Region`; a scripted
  `sl-repl` login per region logged the raw documents first.
- **Findings.** Second Life sends 31 top-level keys, OpenSim 16, and they
  share 14. Second Life alone: the voice backend, a 2048 px texture limit,
  the estate and group limits, the PBR, mirror and pathfinding switches,
  `GLTFEnabled` as `false`, the dead-reckoning pair, the host's name, and —
  differing between the two regions — `LuaScriptsEnabled` and the syntax
  version. OpenSim alone: `menus` (five empty menus) and the 24 keys of
  `OpenSimExtras`, among them `ExportSupported` as a string, a present and
  empty `GridURLAlias`, and the simulator's real frame rate with the factor
  it scales it by. `RenderMaterialsCapability` is a rate (requests a
  second), a real on both grids: 4 and 3. OpenSim answers `404` once the
  agent has logged out.
- **Client.** `SimulatorFeatures` and `OpenSimExtras` gained a typed field
  for every measured key, `DynamicMenus` for `menus`, an `other` map that
  keeps a key no field carries, and `advertised()`. The builder writes each
  key in the kind the grids send it (`ExportSupported` as a string), and two
  unit tests require the two measured documents to re-encode exactly.
- **Fake grid.** `ImitatedGrid::stock_simulator_features`: each flavour's
  whole document, with the fake grid's own addresses, names, currency symbol
  and voice backend put into it. `simulator-features` was already in the
  offline list for both flavours and now holds each to the full table.
- **Viewer.** The existing readers (chat ranges, map and search URLs, the
  presence of the extras block) were checked against both documents and
  read an absent key as their own default. `RenderMaterials` requests are
  now cut to the region's `MaxMaterialsPerTransaction` instead of a fixed
  50, with a unit test.
- **Not done here.** The fake grid does not advertise `LSLSyntaxId` (nor
  Second Life's `LSLSyntaxVersion`): a region advertises it beside a syntax
  document it serves, and the fake grid serves none
  ([[server-fake-grid-lsl-syntax]]). No per-flavour viewer e2e test was
  added: both flavours give the readers above the same values, so there is
  nothing a test could tell apart; the client's decode of each flavour is
  held by the offline case. The keys the reference viewer acts on and ours
  does not are filed as [[viewer-simulator-features-unread-keys]].

## Known already

The extras block is OpenSim-only, `VoiceServerType` SL-only; OpenSim sends
`ExportSupported` as the string `"true"`. The aditi run was deferred; the
fake grid sends four keys.

## Discover

Make `simulator-features` record the whole map; run on aditi and OpenSim; diff.

## Document

`book/src/gridspec/region-arrival.md` § SimulatorFeatures (full per-grid key
table).

## Fake grid

Small — fill each flavour's map with the measured keys and value types.

## Viewer

Limits read per grid, `None` vs `Some(false)`, string booleans; check every
consumer of a key.
