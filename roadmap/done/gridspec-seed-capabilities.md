---
id: gridspec-seed-capabilities
title: Which capabilities each grid grants from the seed, and to whom
topic: gridspec
status: done
origin: user question during gridspec-login (2026-10-04)
refs: [gridspec-survey, gridspec-simulator-features, gridspec-region-arrival,
  gridspec-neighbours-crossing]
---

Context: [context/gridspec.md](../context/gridspec.md).

## Done (2026-10-04)

Measured and written up in `book/src/gridspec/capabilities.md`.

- **Discover.** The tokio client can now be told which capabilities to request
  (`Client::set_requested_capabilities`, an `Arc<[String]>` shared by the
  root, region-change and neighbour seed requests) and reports neighbours'
  capability maps (`Client::set_neighbour_caps_reporter`). The conformance
  `Session` carries both (`relogin_requesting_capabilities`,
  `capability_names`, `neighbour_capability_names`). The bevy client has no
  override: only the conformance harness, which runs on tokio, needs one.
- **The `seed-capabilities` case** logs in with our list, then with ours plus
  Firestorm's 116. aditi refuses only `ObjectAnimation` and
  `UploadBakedTexture` and grants 48 of the 51 reference-only names; OpenSim
  refuses 33 of ours (AIS3, the library fetches, experiences, voice, group
  invites, offline messages, the bake trigger, material saving) and grants 12
  reference-only names, identically on its neighbours. aditi's child grant was
  not measured: the test avatar's start region (Mauve) has no neighbours.
- **Fake grid.** `ImitatedGrid::withheld_capabilities` withholds each grid's
  refused set; `FakeOpensim` was granting AIS3, the library fetches,
  experiences and voice, and `FakeSl` `UploadBakedTexture`. The case holds live
  grids to exactly the measured refusals and fakes to at least them.
  `asset-round-trip` now records a class with no update capability instead of
  failing, and two `client_end_to_end` tests assert the measured shapes.
- **Viewer.** An audit of both runtimes for every refused capability found
  three silent gaps, fixed and tested: a user report falls back to the UDP
  `UserReport`, `RequestOfflineMessages` to `RetrieveInstantMessages`
  (exercised on live OpenSim by `offline-msg-fetch`), and a material edit
  without `ModifyMaterialParams` is reported rather than dropped. The rest —
  experiences, voice, bevy's pre-capability window — is
  [[viewer-refused-capability-silent-drops]]. The capabilities only the
  reference viewer requests are [[protocol-reference-capabilities]].

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
