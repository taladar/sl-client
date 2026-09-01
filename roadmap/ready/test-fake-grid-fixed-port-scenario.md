---
id: test-fake-grid-fixed-port-scenario
title: Named scenarios and a fixed-port launcher for the fake grid
topic: test
status: ready
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
