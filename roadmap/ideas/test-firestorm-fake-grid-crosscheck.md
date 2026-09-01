---
id: test-firestorm-fake-grid-crosscheck
title: Point Firestorm at the fake grid to calibrate oracles
topic: test
status: ideas
origin: test-harness plan (2026-08-30); the "Firestorm smoke" follow-up of the fake-grid series
points: 2
refs: [viewer-fake-grid-login-smoke, test-fake-grid-render-fixtures, test-fake-grid-fixed-port-scenario, test-firestorm-crosscheck-report]
---

Context: [context/testing.md](../context/testing.md).

Firestorm's snapshots answer **calibration** questions about the fixtures
themselves — what "terrain" reads as at the chosen sun angle, whether the
checker is legible at the fixture's distance, whether the border line is
visible, how many avatars survive a crossing. Those answers are recorded
as prose in the fake-grid book chapter, because what they pin down is what
a fixture *should look like*, which is a sentence, not an image.

One precondition is already met (2026-09-01): the fake grid's stock region
environment used to be an **empty** day cycle, which says nothing about the
sky, so each client rendered its own built-in default and any question about
"the chosen sun angle" had two different answers. `default_region_environment`
now serves a real single-keyframe cycle carrying the reference's own default
sky and water — wire-determined, and independent of the region clock, so two
snapshots taken minutes apart are comparable too.

The launcher this needs is now its own task,
[[test-fake-grid-fixed-port-scenario]].

Nothing here is a reference image, and this task never grows an image
comparison: an oracle calibrated against a stored screenshot is calibrated
against one machine's driver. That constraint is about **this** task and
about the `cargo nextest` tiers, not about tooling in general — the
developer-facing divergence hunt in
[[test-firestorm-crosscheck-report]] does diff images, deliberately, and
deliberately stays outside the test suite.
