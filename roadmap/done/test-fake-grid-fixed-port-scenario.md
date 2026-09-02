---
id: test-fake-grid-fixed-port-scenario
title: Named scenarios and a fixed-port launcher for the fake grid
topic: test
status: done
origin: Firestorm cross-check harness plan (2026-09-01); the unbuilt half of test-firestorm-fake-grid-crosscheck
points: 2
refs: [test-fake-grid-render-fixtures, test-firestorm-fake-grid-crosscheck]
---

Context: [context/testing.md](../context/testing.md).

The `sl-fake-grid` binary already takes `--http-port`, `--account`,
`--region` and `--catalogue`, which is enough to log a viewer in by hand.
A harness that starts the grid, runs two viewers against it and compares
the results needs two more things.

`--scenario <name>` selecting from the shared fixture catalogue, so "the
scene both viewers photographed" is a name in the repository rather than a
command line someone retyped. `catalogue` is the first scenario; the point
of the flag is that the next one does not change the harness.

A launcher script that starts the grid on a fixed port and prints the
login URI as an **IPv4 literal**. `localhost` resolves to `::1` first and
the fake grid listens IPv4-only, so a viewer told `localhost` fails to
connect for a reason that looks nothing like the cause. Firestorm in
particular takes the URI's host:port as the grid name and fetches
`GET /get_grid_info` from it.

The port must be fixed rather than ephemeral: both viewers are configured
before either starts, and Firestorm caches the grid in its grid manager
between runs.

Refs [[test-fake-grid-render-fixtures]] for the catalogue this selects
from. This is the half of [[test-firestorm-fake-grid-crosscheck]] that is
still unbuilt; the calibration half stays there.

Done (2026-09-02): `sl-fake-grid/src/fixtures/scenarios.rs` is the
registry — `NamedScenario { name, summary }` with `dress(region)` and
`landmarks()`, plus `all()` / `names()` / `scenario(name)` and
`DEFAULT`. Two scenes: `stock` and `catalogue`. The binary's
`--catalogue` flag became `--scenario <name>`, whose possible values come
from `names()`, so naming the next scene needs no change to the binary,
its help, or the launcher.

A scenario is *how it dresses a region* (`fn(RegionConfig) ->
RegionConfig`), not a `RegionFixture`, for one reason worth keeping: the
stock scene is not a fixture at all. It is the grid-wide
`Scenario::default` a region with no scenario of its own inherits, and
flattening it into a `RegionFixture` would silently drop the arrival
greeting and the legacy UDP asset fixtures that come with it.

`landmarks()` was not in the plan and earns its place twice: the binary
now logs what stands in *whatever* scene was chosen instead of
special-casing the catalogue, and the runner can aim a camera at "the
landmark called `mesh-cube`" rather than at a hard-coded position.

`scripts/fake-grid.sh` is the launcher: `--port` (default 9100),
`--scenario` (default `catalogue`), `--debug`, everything else passed to
the binary. It builds first (a compile error reads as a compile error,
not as a grid that never came up), waits until the grid answers
`get_grid_info`, and prints the login URI as an IPv4 literal beside the
`--grid 127.0.0.1:<port> --multiple` form Firestorm wants. Its scenario
default is `catalogue` while the binary's is `stock`, deliberately: a
launcher run is a cross-check or a hand-driven Firestorm session and both
want the feature row, while the bare binary should keep behaving as it
did. The banner names the scene it started, so neither default has to be
remembered.

Two failure modes cost a fix each while writing it, and are worth
knowing before writing the runner:

- **A readiness probe proves the port answers, not that *your* grid
  does.** The first version started the grid, polled `get_grid_info`, and
  happily printed "ready" while the process it had just started was dying
  of `AddrInUse` — a leftover grid from an earlier run was answering.
  The launcher now refuses a port anything already answers on (curl's
  exit 7, "could not connect", is the only answer that means free).
- **`kill -0` says yes to a zombie.** A child that has exited and not
  been reaped still passes the liveness check, so a grid that died during
  startup looked alive for the whole timeout. The launcher checks the
  process state as well.
