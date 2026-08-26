---
id: test-audit-fake-grid-conformance-grid
title: Teach sl-conformance about sl-fake-grid so ~16 cases run offline
topic: test
status: ready
origin: static code audit (2026-08-26)
points: 8
refs: [build-audit-ci-pipeline]
---

Context: [context/test.md](../context/test.md).

`sl-conformance/src/grid.rs:9-15` has exactly two variants, `Opensim` and
`Aditi`, and `sl-conformance` does not depend on `sl-fake-grid` at all.

Meanwhile `sl-fake-grid` (4210 lines) already ships `login_endpoint`,
`caps_endpoint`, `economy_endpoint`, `udp_assets` (Xfer, task inventory,
`TransferRequest`, estate terrain RAW), `world` (region burst, parcel overlay,
prims), `teleport`, `map_tiles` and a `FakeGridBuilder` on ephemeral ports — the
whole surface these cases exercise.

Adding `Grid::Fake` plus one branch in `context::connect_and_spawn` is the
single highest-value change in the crate. Cases that assert only protocol shape,
with every fixture already present, and could then run as plain `cargo test`:

`login-handshake`, `keepalive-ping`, `throttle-set`, `economy-data`,
`simulator-features`, `object-update-decode`, `parcel-properties`,
`parcel-info-dwell`, `terrain-raw-download`, `task-inventory`,
`map-blocks-items`, `teleport-local-phases`, `teleport-cross-region`,
`texture-fetch-http`, `asset-fetch-http`, `logout-clean`, `agent-alert`,
`server-error`.

That is roughly 16 of 98 cases — and they are exactly the ones a regression
would break silently today, because nobody runs them between manual grid
sessions. Everything asserting *grid semantics* rather than *wire shape*
(group, estate, money, experience, display-names, offline-IM, marketplace,
AIS3, and the 17 cases pinned to the OpenSim OAR fixture) stays live.

Offline behaviours the fake grid already serves that nothing tests today: a cap
fetch with a `Range` header, `FakeGridBuilder::gates` (`runtime.rs:625`) and
`AccountConfig::mfa` (`accounts.rs:22`) — the whole ToS / critical-message /
already-logged-in / MFA login matrix — and the teleport **arrival-timeout**
branch (`teleport.rs:165-181`), which is what the teleport-progress watchdog
depends on.
