---
id: test-fake-grid-imitates-economy
title: The fake grid's money is a stock OpenSim region's on either flavour
topic: test
status: ready
origin: auditing the divergences while doing test-fake-grid-object-asset-id-divergence (2026-09-07)
points: 2
refs: [test-fake-grid-object-asset-id-divergence]
---

Context: [context/testing.md](../context/testing.md).

`EconomyConfig` is already a builder knob, so unlike the other three
divergences this one needs no new behaviour to become flavour-decided — it
needs a **measurement**.

`stock_prices` is documented as the zeroes a stock OpenSim region answers with
(`SampleMoneyModule`), and that is what both flavours answer today. What Second
Life's `EconomyDataRequest` actually returns — the upload charges, the group
creation fee, the object-count limits — is not written down anywhere in this
workspace, so a Second-Life-flavoured default cannot be written honestly yet.

Two steps, in order:

- Measure it. A conformance case already logs into aditi; the reply is one
  `EconomyData` message and `economy-data` is registered on both live grids
  already, so what is missing is a record of the aditi numbers rather than new
  machinery.
- Then `ImitatedGrid` picks the price list, and the currency symbol with it
  (`L$` on both, but the helper flow behind it is not the same shape).

Worth keeping small: nothing a viewer *does* depends on these numbers being
right, only what it *displays*. It is on the list because a grid that says it
is Second Life and quotes a stock OpenSim region's zeroes is the same category
of lie as the other three, not because anything is blocked on it.

Acceptance: the aditi `EconomyData` numbers are recorded, `ImitatedGrid` picks
the price list, and `economy-data` asserts the flavour's own answer rather than
whichever one happened to be the default.
