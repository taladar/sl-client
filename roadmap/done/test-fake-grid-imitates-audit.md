---
id: test-fake-grid-imitates-audit
title: The fake grid was nobody in particular
topic: test
status: done
origin: review of test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 3
refs:
  [
    test-fake-grid-object-asset-id-divergence,
    test-fake-grid-imitates-inventory-api,
    test-fake-grid-imitates-server-bakes,
    test-fake-grid-imitates-simulator-features,
    test-fake-grid-imitates-economy,
  ]
---

Done 2026-09-07.

Context: [context/testing.md](../context/testing.md).

[[test-fake-grid-object-asset-id-divergence]] gave the fake grid a switch for
*one* divergence and answered it with a knob a conformance case set. The review
of it made the point the knob had hidden: which live grid the fake one imitates
is not one behaviour's business, and there are several more divergences that
each picked a side on their own.

The state that made the case: a stock fake grid announced `platform: OpenSim`,
kept every login field like OpenSim (`honor_options: false`), and withheld a
taken object's asset like Second Life — all at once. **A viewer passing against
that has not been tested against anything.**

## What landed

`imitates::ImitatedGrid` is the fake grid's top-level setting
(`FakeGridBuilder::imitates`, default Second Life), and each divergent knob is
an `Option` resolved once at `start`: an explicit setter wins, anything unset is
whatever the imitated grid does. Two behaviours follow it today — a taken
object's asset, and whether the login response is trimmed to the request's
`options`. Its module doc carries the **audit**: what it decides, what it does
not decide and what the other side would cost, one roadmap item each
([[test-fake-grid-imitates-inventory-api]],
[[test-fake-grid-imitates-server-bakes]],
[[test-fake-grid-imitates-simulator-features]],
[[test-fake-grid-imitates-economy]]) — plus the one thing that is deliberately
not flavour-decided and is not a to-do (`GridIdentity::platform` stays
`OpenSim`, because it is what Firestorm's grid manager reads to decide whether
it will add the grid at all).

In the harness the flavour is **the grid**, not a setting on the run:
`Grid::Fake` became `Grid::FakeSl` and `Grid::FakeOpensim`. A case declares the
flavours it is meaningful on in `grids()` the way it declares it is meaningless
on aditi, `run_offline_case` runs it once per flavour it declared, and
`FakeGridHarness::start` takes the grid. So the per-case knob went away
entirely. Naming both rather than only the odd one out is the point: a grid
called plainly "fake" reads as flavourless, which is how this happened.

Two cases stopped being flavour-agnostic, and they are the two shapes to copy:
`asset-round-trip` names `Grid::FakeOpensim` alone (reading a taken object's
asset back is something only OpenSim allows, and the case should not run on the
other and claim to have tested it), and `object-asset-format` names **both**,
because it is a survey of exactly what the two disagree about and both answers
are worth having.

## What it caught immediately

Deriving `honor_options` flipped the stock grid to Second Life's side and broke
a fake-grid end-to-end test that expected `map-server-url` in the login
response. The test was right and the **client** was wrong: `LoginRequest::new`
asked for six options and consumed a seventh, so against Second Life — which
honours the list — the grid's map-tile server URL would never have arrived, and
the viewer would have fallen through to its CDN default without anything
failing. Fixed by asking for it, with the rule written down beside the list:
ask for everything read back.

That is the whole argument for the flavour in one bug. Nothing was wrong with
the fake grid; what was wrong was that it had never committed to being one real
grid, so a client gap only Second Life would expose had nowhere to show up.
