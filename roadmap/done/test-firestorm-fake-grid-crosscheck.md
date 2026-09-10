---
id: test-firestorm-fake-grid-crosscheck
title: Point Firestorm at the fake grid to calibrate oracles
topic: test
status: done
origin: test-harness plan (2026-08-30); the "Firestorm smoke" follow-up of the fake-grid series
points: 2
refs: [viewer-fake-grid-login-smoke, test-fake-grid-render-fixtures, test-fake-grid-fixed-port-scenario, test-firestorm-crosscheck-report, test-fake-grid-sky-without-density-profiles, test-crosscheck-day-position-is-inert]
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

**That precondition was met on the wire and not in the viewer**, which is the
first thing the first Firestorm run found: the reference rejected the cycle
outright for want of three required keys, and had been lighting the grid with
its own default sky all along — [[test-fake-grid-sky-without-density-profiles]],
fixed here.

The launcher this needs is now its own task,
[[test-fake-grid-fixed-port-scenario]].

Nothing here is a reference image, and this task never grows an image
comparison: an oracle calibrated against a stored screenshot is calibrated
against one machine's driver. That constraint is about **this** task and
about the `cargo nextest` tiers, not about tooling in general — the
developer-facing divergence hunt in
[[test-firestorm-crosscheck-report]] does diff images, deliberately, and
deliberately stays outside the test suite.

## Done (2026-09-10)

The calibration prose is in the fake-grid book chapter, under "Calibration:
what these fixtures look like in the reference viewer": what the ground reads
as and why its colour is not a height reading, where the stock sky's sun
actually stands (80° up, with stars, which is a signature and not dusk), how
big the checker's cells land at the fixture's own framing, that a border is
legible only because the grounds are painted, that the neighbour is already on
the horizon before you cross, and that a crossing ends with two avatars rather
than three.

Two of the four questions could not be *staged* by the harness as it stood,
and the harness grew what they needed rather than the questions being dropped:

- `sl-crosscheck --neighbour` stands up the scene's second half one slot east
  and lets the grid announce it, and `--cross-after <seconds>` walks the agent
  over that border with a one-step `Timeline`. A scene now says how it dresses
  a **pair** of regions (`NamedScenario::pair`), because two halves of a
  border are not two copies of one scene;
- `fixtures::border::border_pair_side` is that dressing: painted ground,
  ridden vehicle, marker pillar.

Two findings came out of it beyond the prose. The sky one is above. The other
is the fixture flaw the pair scene exposed: the marker pillar had **one**
grid-wide id for both regions, and a viewer keys an object by that id, so the
pair had a single pillar being moved back and forth rather than one each.
Split per side (`BorderSide::marker_object`); the vehicle and rider keep a
shared id on purpose, which is what makes a handover a handover.

Left open, deliberately, as [[test-crosscheck-day-position-is-inert]]:
`--day-position` still cannot choose a sun angle, because the harness looks
for a frame at exactly that keyframe and the stock cycle has one at `0.0`.
The calibration above is therefore of the one sky the fixture pins, which is
the sky every capture of it is taken under.
