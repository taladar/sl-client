---
id: viewer-ui-baseline-regressions
title: UI regression baselines — recorded geometry that may not drift by accident
topic: viewer
status: ready
origin: the viewer-ui-test-harness work (2026-07); the third check tier that task identified but did not build
blocked_by: [viewer-ui-test-harness]
refs: [viewer-ui-radial-menu, viewer-render-test-harness, viewer-ui-skin-tokens]
---

Context: [context/viewer.md](../context/viewer.md).

The third tier of the UI check suite. [[viewer-ui-test-harness]] built two:

| Tier | Question | Who decides |
| --- | --- | --- |
| **Universal** | is this broken? | the harness, for every element, no opt-in |
| **Declared** | does this match its stated intent? | the element (`AlignmentGroup`) |
| **Baseline** | *has this moved?* | **a committed recording — this task** |

The first two only catch things that are **wrong**. The gap: properties that are
not wrong at any particular value, but must not change *by accident*.

## Why this is not just more assertions

The motivating case is the **pie menu** ([[viewer-ui-radial-menu]]). A Second
Life user has opened the same radial menu tens of thousands of times, and their
hand knows where Sit is without their eyes. Nothing is *incorrect* if Sit moves
from 45° to 60° — no invariant is broken, no content overflows, every check in
the suite stays green — and the user's muscle memory is gone. An angle nobody
declared, that no invariant protects, that a refactor can move for free, and
that is expensive to move.

The same shape applies well beyond it: a floater's default size, a toolbar
button's order, a chiclet's position, the spacing between the widgets a user
double-clicks between. Cheap to change accidentally, costly to have changed.

## What it is

Record the geometry — angles, positions, sizes, order — of **named** things into
a **committed** baseline file. Compare on every run. A difference fails, and the
only way to change one is to update the baseline **deliberately**, in the same
commit, where a reviewer sees it.

The value is entirely in that review moment. "This diff moves the Sit option
12°" is a sentence somebody can object to *before* it ships; the same change
buried in a layout refactor is one nobody sees until a user complains that the
menu feels wrong and cannot say why.

## The design constraints, learnt from the first two tiers

- **Opt-in, not universal.** Baselining everything would make every deliberate
  layout change a noisy diff, and a noisy check gets ignored and then deleted —
  the same failure the clipping check nearly had (see `TextMayClip`). Only what
  is *load-bearing for muscle memory* gets a baseline, and it says why.
- **Record derived intent, not raw pixels.** Baseline the *angle* of a pie
  option, not its `ComputedNode.size`. A raw geometry dump changes whenever a
  font is updated and teaches everyone to re-bless the file without reading it,
  which is strictly worse than no check.
- **One canonical cell, not the matrix.** [[viewer-ui-test-harness]]'s matrix is
  8 scripts × 3 sizes × 3 scales × 2 directions; baselining every cell would be
  an unreadable file and a permanent merge conflict. Baseline the resting cell,
  and let the universal tier cover the rest. A pie option's *angle* should not
  depend on the language anyway — and if it does, that is a finding.
- **Reuse the registry.** `ELEMENTS` in
  `sl-client-bevy-viewer/src/ui_element.rs` is already the one list; a baseline
  is keyed by element id and node `Name`.

## Prior art in this workspace

`records/` + `sl-conformance-report` already does exactly this shape for the
protocol: a committed, git-stamped recording, compared against the current
commit, refreshed deliberately. Read that first — the mechanism is proven here
and the conventions (what to commit, how to re-bless, how to report staleness)
should not be reinvented.

[[viewer-render-test-harness]] needs the same tier for 3D and should share this
one's format rather than growing a second.
