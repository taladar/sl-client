---
id: gridspec-region-info
title: RegionInfo, region flags and limits on each grid
topic: gridspec
status: done
origin: gridspec survey (2026-10-03)
refs: [gridspec-survey, protocol-region-flag-deny-ageunverified-value,
  viewer-script-limits]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Known already

`RegionInfo5` is newer-SL-only; `OpenRegionInfo` OpenSim-only; the fake grid
answers `RequestRegionInfo` from defaults, not its store (stale-read bug).

Audit 2026-10-15: the stale read is not visible in the code. The fake grid's
`RequestRegionInfo` arm (`sl-fake-grid/src/parcel_edits.rs`) answers from
`SceneFixtures::limits` (`sl-fake-grid/src/world.rs`), which returns the
edited limits once an estate manager has changed them (since 2026-09-05), and
an edit is pushed as `RegionChange::RegionConfigured`. Pin it with a test
(edit, then request) before fixing anything; if it holds, only the flavour
defaults remain for the fake grid here.

## Discover

Extend `region-info` to record every block and field on both grids (no rights
needed).

## Document

`book/src/gridspec/estate.md` § Region info.

## Fake grid

Small — flavour defaults and the stale read in this task.

## Viewer

Absent blocks, `u64` flags, status-bar icons.

## Capabilities done in this task

[[protocol-cap-dispatch-region-info]]: region settings over
`DispatchRegionInfo` (Second Life), including the two flags UDP cannot
carry. Also the protocol half of `RegionSchedule` (read and write the restart
schedule; the floater is [[viewer-region-restart-schedule]]).

## Done (2026-10-17)

The census `region-info` (2av, all four grids) records the handshake and the
`RegionInfo` field by field and block by block as each avatar reads them, the
world map's rating, the raw answers of `DispatchRegionInfo` and
`RegionSchedule`, a resident's refusals over both and over UDP, and — where A
holds estate rights — every Region-tab setting flipped, read back and
restored, and scripts switched off and on over the Debug tab, recording what
each avatar is told. Held to `Measured` constants (region configuration
recorded, not held); written up in `book/src/gridspec/estate.md` § Region
info.

- **Both grids** answer `RequestRegionInfo` to anybody and tell nobody else;
  a Region-tab save pushes a `RegionInfo` to everybody, a Debug-tab save is
  told only in the next `SimStats`, and neither re-sends the handshake.
- **OpenSim** sends no `RegionInfo3`, chat or combat block, and its record and
  `SimStats` state the region's flags without the estate's bits; grants
  neither capability.
- **Second Life** sends every block (chat 10/20/100, combat `0x5`), the
  handshake's flags, `-1,-1` for no redirect; refuses a resident's
  `DispatchRegionInfo` and `RegionSchedule` writes `403`, and reads an empty
  schedule `undef`. Mauve's handshake says `Mature` where its record and the
  map say `Adult`. The manager's legs wait on [[gridspec-aditi-test-land]].

The stale read the survey suspected does not exist (the census reads every
change back on all four grids).

Client: `RegionFlags` is the reference's one 64-bit space
([[protocol-region-flag-deny-ageunverified-value]]: `DENY_AGEUNVERIFIED` is bit
30, every `REGION_FLAGS_*` named, `EstateFlags` merged into it);
`RegionIdentity`, `RegionLimits` and `RegionStats` keep the legacy field and
the extended one as `Option` (absent block = `None`, `flags()` prefers it), and
`RegionLimits` gains `RegionInfo2`'s product and the legacy agent cap. The
session keeps a region's flags as last stated by handshake, `RegionInfo` or
`SimStats`. `DispatchRegionInfo` adopted ([[protocol-cap-dispatch-region-info]])
and the protocol half of `RegionSchedule` (`Command::RequestRegionSchedule`,
`Command::SetRegionSchedule`, `Event::RegionSchedule`, codecs in
`sl-wire/src/region_caps.rs`), both runtimes and the REPL.

Fake grid: `RegionInfoPolicy` (extended block, region-only flags, combat
block, no-redirect coordinates), both capabilities served on `FakeSl` and
withheld on `FakeOpensim` with Second Life's refusals, `setregioninfo` now
applies the flags too, `setregiondebug` answered, `SimStats` restating the
flags after a change, no estate sun. e2e: `a_region_save_keeps_what_each_grids_
road_carries`, `a_debug_save_is_told_by_sim_stats_alone`, `the_restart_
schedule_round_trips_for_a_manager_only` (`client_end_to_end.rs`).

Viewer: `SlRegionFlags` (handshake, `RegionInfo`, `SimStats`) drives the status
bar's icons; the Region tab gains the fly-over and search boxes, greyed where
`DispatchRegionInfo` is not granted; an Apply saving a rating other than the
handshake's raises `RegionMaturityChange` as the reference's does. e2e:
`the_region_tab_follows_each_grids_capability_and_rating` (`e2e_windows.rs`).
