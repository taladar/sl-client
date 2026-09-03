---
id: test-audit-fake-grid-conformance-grid
title: Teach sl-conformance about sl-fake-grid so ~16 cases run offline
topic: test
status: done
origin: static code audit (2026-08-26)
points: 8
refs: [build-audit-ci-pipeline, test-fake-grid-render-fixtures]
blocked_by: [test-fake-grid-determinism]
---

Done 2026-09-03. **Eighteen** cases now run as plain `cargo test`.

Context: [context/testing.md](../context/testing.md), and the book's
*conformance testing* chapter, whose runner page gained a
[The offline grid](../../book/src/conformance/runner.md) section stating the
two rules for what belongs there.

`Grid::Fake` is a third variant; `sl_conformance::fake` stands the grid up
(the catalogue region plus the border scene east of it, announced as its
neighbour), registers three accounts and synthesises the `sl-repl` credentials
that reach the ephemeral port it bound — so the whole login path below it,
XML-RPC round trip included, is the one a live grid takes. The single branch
the task predicted in `connect_and_spawn` is real but smaller than expected:
`Grid::default_login_uri` became an `Option`, because the fake grid is the one
grid with no fixed address.

`sl-conformance/tests/offline.rs` is the point of all of it: one
`#[tokio::test]` per name in `fake::OFFLINE_CASES`, each on its own fresh grid,
so a failure names its case and a case that mutates a region cannot decide what
the next one sees. A unit test pins the list against the registry **in both
directions** — a case cannot declare `Grid::Fake` without being run, nor be
listed without declaring it. Nothing offline writes a record: the assertion is
re-made every run, so a committed copy could only be staler, which is why
`Grid::ALL` became `Grid::RECORDED` and stayed the two live grids.

The four cases handed over by [[viewer-fake-grid-render-catalogue]] all landed,
and all four are `&[Grid::Fake]` only:

- `region-crossing` — the client promotes the child circuit it already held
  (`world_reset: false`, a different circuit than the source's, the simulator
  the neighbour was announced at) and raises no teleport event doing it. It is
  also the case that needed a new seam: a crossing is a decision a *region*
  makes, and this grid simulates no movement, so `TestContext::fake()` hands a
  case the grid-side `FakeControl` to make it with. `None` on a live grid.
- `neighbour-child-circuits` — announced (`NeighborDiscovered`), seeded
  (`NeighborSeed` for the same simulator), streaming (an object stamped with
  the *neighbour's* handle, which is the border scene's marker pillar).
- `terrain-layerdata` — all 256 land patches, each index exactly once, each a
  full 16×16 grid of decoded heights within a tenth of a metre of the
  fixture's. The codec is lossy, so the tolerance is the test's, not a fudge.
- `avatar-appearance-npc` — the catalogue NPC's bakes are the fixture's ids in
  the fixture's slots, its visual params are the fixture's bytes, and the first
  bake is *fetched* over `GetTexture` — a named bake nobody serves is a cloud
  with extra steps.

**One fake-grid feature had to be built to get there: the world map.**
`sl-fake-grid/src/world_map.rs` answers `MapBlockRequest`, `MapNameRequest`,
`MapItemRequest` and `MapLayerRequest` from a catalogue built once from the
region table and shared by every session — right in more than one sense, since
a viewer asks whichever simulator it is on about the whole grid. Both sides
already existed in `sl-proto` (the `ServerEvent`s and the `send_map_*_reply`
helpers); nothing had ever wired them together. That unlocked
`map-blocks-items` and, with it, `teleport-cross-region`, which *discovers* its
destination through a map query. It also means a viewer pointed at the
standalone binary now has a world map at all.

Four of the eighteen names the task predicted are **not** in the list, and each
for a stated reason rather than an oversight:

- `economy-data`, `parcel-info-dwell`, `asset-fetch-http` and `task-inventory`
  each need a simulator-side surface `sl-proto` does not have — no
  `EconomyDataRequest`, `ParcelDwellRequest` or `AgentWearablesRequest` reaches
  a `ServerEvent`, and object rez / derez / task-inventory-update is a
  simulator, not a fixture. Handed to
  [[test-fake-grid-simulator-request-surfaces]].
- `agent-alert` and `server-error` *pass* offline, and were taken back out
  again: both provocations are OpenSim-estate-specific admin nudges the fake
  grid has no answer for, so each records `partial` after burning its whole
  reply window — ninety seconds of suite time to assert nothing. They stay
  live-only until the grid can answer them.

Two smaller things fell out of the work:

- `support::content_is_ours` (OpenSim **or** fake, not aditi) replaces
  `is_opensim` wherever a case branched on "is the region's content something
  this workspace declares" to decide between a `check` and a `mark_partial`.
  `object-update-decode` and `simulator-features` now hold the fake grid to the
  same claim they hold OpenSim to.
- `Fixtures::default_path` became an `Option` too: the fake grid *is* the
  fixture, so there is no file for it to point at.

The remaining offline behaviours the task listed as untested — the
`Range`-header cap fetch, `FakeGridBuilder::gates` / `AccountConfig::mfa` (the
ToS / critical-message / already-logged-in / MFA login matrix), and the teleport
arrival-timeout branch the progress watchdog depends on — are untouched and
still worth a case each; they are in
[[test-fake-grid-login-matrix-and-timeouts]].
