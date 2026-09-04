---
id: test-fake-grid-simulator-request-surfaces
title: Four client requests no simulator half answers
topic: test
status: done
origin: doing test-audit-fake-grid-conformance-grid (2026-09-03)
points: 5
refs: [test-audit-fake-grid-conformance-grid, test-fake-grid-object-write-path]
---

Done 2026-09-04, except the object write path, which is now
[[test-fake-grid-object-write-path]] — the split this file already sanctioned.
Five cases joined `OFFLINE_CASES` (`economy-data`, `parcel-info-dwell`,
`asset-fetch-http`, `agent-alert`, `server-error`), taking it from sixteen to
twenty-one.

Context: [context/testing.md](../context/testing.md).

[[test-audit-fake-grid-conformance-grid]] took eighteen conformance cases
offline. Four more were on its list and could not go, and all four fail the
same way: `sl-proto`'s `SimSession` decodes the client's request and then has
nowhere to put it — no `ServerEvent`, no `send_*` counterpart — so no grid
built on it can answer, and the fake grid least of all.

Each is a small, symmetric addition to the simulator half (the pattern is
`AnyMessage::X(request) => events.push_back(ServerEvent::Y { … })` plus a
`SimSession::send_z`, exactly as `ParcelInfoRequest` /
`send_parcel_info_reply` already are), followed by one arm in the fake grid's
`answer_world_request` and one name added to `fake::OFFLINE_CASES` and
`tests/offline.rs`:

- **`EconomyDataRequest` → `EconomyData`.** The grid-wide prices and the
  region's object capacity. `EconomyData` the *type* and its wire message both
  exist; only the server direction is missing. Unlocks `economy-data`. The
  fake grid already has an `EconomyConfig` for the HTTP helpers, which is the
  natural place to state the prices.
- **`ParcelDwellRequest` → `ParcelDwellReply`.** Nothing in `sim_session.rs`
  mentions `ParcelDwell` at all. With this plus the `ParcelInfoRequest` arm (the
  send half of which already exists) and the parcel-cover rectangles
  `SimSession::resolve_remote_parcel` matches against — the
  `RemoteParcelRequest` capability is implemented but the fake grid registers no
  covers — the whole of `parcel-info-dwell` runs offline.
- **`AgentWearablesRequest` → `AgentWearablesUpdate`.** The stock scenario
  already dresses the account in four body parts and files the COF links; it
  simply cannot *say so* when asked. Unlocks the first half of
  `asset-fetch-http`. The second half needs the named wearable assets to be
  fetchable over `ViewerAsset`, and the ids the stock scenario uses are Linden
  library ones a viewer resolves locally and never asks for — so the fake grid
  either serves a stand-in per id (the precedent
  [[test-fake-grid-builtin-textures]] set) or the fixture mints its own.
- **`ObjectAdd` / `DeRezObject` / `UpdateTaskInventory`.** The largest by far,
  and the only one that is a *simulator* rather than a fixture: rezzing an
  object, taking it into agent inventory, and copying an item into a prim's
  task inventory with the contents serial advancing. Unlocks `task-inventory`,
  and would be the first grid-side write path the fake grid has. Worth
  splitting out if the other three land first.

Two more cases already run offline but assert nothing there, and belong to this
task rather than to a fifth one: `agent-alert` and `server-error` were taken
back out of `OFFLINE_CASES` because each records `partial` after burning its
whole reply window. `agent-alert`'s Set-Home half needs
`ServerEvent::SetStartLocation` answered with an `AgentAlertMessage` (both ends
exist; nothing joins them), and its estate half plus `server-error`'s
deprecated-fetch refusal need the fake grid to model an estate-rights check and
a `FeatureDisabled` policy respectively.

Acceptance: each request that a `SimSession` decodes has somewhere to go, the
fake grid answers it, and the case that needed it is in `OFFLINE_CASES` with
its test in `tests/offline.rs`.

## What landed

Three `ServerEvent`s and three `send_*` on `SimSession`, and the three names
came off the `RAW_FORWARDED` ledger (which fails the family test that still
sends them, so their assertions moved into `tests/sim_session.rs`). The
`ParcelDwellRequest` event carries only the **region-local** id: the template
marks the request's `ParcelID` "filled in on sim", so a viewer sends it nil.

The fake grid's side turned out to be less about the three arms than about the
state behind them, and each answer is derived rather than restated:

- **A parcel is stated once, with both halves.** `SceneFixtures::add_parcel`
  takes the region-local record *and* a `ParcelListing` (grid-wide id, dwell),
  because three surfaces want the id the `ParcelProperties` record has no field
  for. The runtime registers one `SimParcel` cover per listing from the
  parcel's own bounds — the `add_parcel` that used to sit in `default_setup`
  hard-coding the stock parcel is gone — and the search listing a
  `ParcelInfoRequest` answers with is derived from the pair, so the record and
  the listing cannot drift into describing different land.
- **The outfit is simulator state, not a folder read.**
  `SimSession::set_agent_wearables` holds it (advancing the serial itself,
  since a receiver drops an update whose serial did not move), seeded by
  `default_setup` in the same loop that files the COF links. The four body-part
  ids moved to `sl_test_assets::builtin::DEFAULT_BODY_PARTS`, so the items, the
  worn set and the served assets cannot name different ids.
- **The library body parts are now served.** `builtin::library_wearables`
  writes an `LLWearable` stand-in per id through `sl-avatar`'s own
  `WearableAsset::to_text` (the inverse of the parser a bake reads them with),
  naming exactly the layer textures `WEARABLE_LAYER_TEXTURES` lists and
  `library_textures` already serves. A viewer that ships them still answers
  them locally; `asset-fetch-http` does not, which is what makes its second
  half real.

`agent-alert` and `server-error` needed policy rather than fixtures, so
`agent_requests.rs` is a third answerer next to the world and map ones, over an
`AgentPolicy`: `AccountConfig::estate_manager` (false by default — OpenSim
drops an estate command an agent has no power for without a word, so the check
is a check, and the conformance grid marks only its primary, exactly as a live
OpenSim run must) and `FakeGridBuilder::legacy_udp_inventory` (defaulting to
`Refused`, because of the two answers a grid that does not serve UDP inventory
can give only `FeatureDisabled` is observable). Set-Home follows OpenSim's own
rule and its own two strings — the land's owner may set home on it and nobody
else may — so on the catalogue region an ordinary avatar gets the refusal, and
*some* alert always arrives, which is what makes Set-Home the deterministic way
to provoke one.
