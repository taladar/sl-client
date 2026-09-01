---
id: test-firestorm-fake-grid-crosscheck
title: Point Firestorm at the fake grid to calibrate oracles (never to diff images)
topic: test
status: ideas
origin: test-harness plan (2026-08-30); the "Firestorm smoke" follow-up of the fake-grid series
points: 2
refs: [viewer-fake-grid-login-smoke, test-fake-grid-render-fixtures]
---

Context: [context/testing.md](../context/testing.md).

Add `--scenario <name>` to the `sl-fake-grid` binary, selecting from the
shared fixture catalogue, and a script that starts it on a fixed port and
prints the grid-manager URI (the IPv4 literal — `localhost` resolves to
`::1` first). Firestorm's snapshots then answer calibration questions —
what "terrain" reads as at the chosen sun angle, that the checker is
legible at the fixture's distance, that the border line is visible, how
many avatars survive a crossing — recorded as prose in the fake-grid book
chapter, never as reference images.

One precondition is already met (2026-09-01): the fake grid's stock region
environment used to be an **empty** day cycle, which says nothing about the
sky, so each client rendered its own built-in default and any question about
"the chosen sun angle" had two different answers. `default_region_environment`
now serves a real single-keyframe cycle carrying the reference's own default
sky and water — wire-determined, and independent of the region clock, so two
snapshots taken minutes apart are comparable too.
