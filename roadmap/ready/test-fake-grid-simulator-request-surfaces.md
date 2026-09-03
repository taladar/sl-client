---
id: test-fake-grid-simulator-request-surfaces
title: Four client requests no simulator half answers
topic: test
status: ready
origin: doing test-audit-fake-grid-conformance-grid (2026-09-03)
points: 5
refs: [test-audit-fake-grid-conformance-grid]
---

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
