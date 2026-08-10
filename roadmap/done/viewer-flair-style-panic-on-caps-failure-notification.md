---
id: viewer-flair-style-panic-on-caps-failure-notification
title: bevy_flair debug panic — recalculate_style on a freshly spawned
  (Reset) widget
topic: viewer
status: done
origin: terse-update fast-path live verification (2026-08-10, local OpenSim)
refs: []
---

Context: [context/viewer.md](../context/viewer.md).

Debug-build viewer logins crashed seconds after login:

```text
bevy_flair_style-0.8.0/src/systems.rs:516:20:
Cannot set next system to NeedsCalculateStyle because current state is Reset
```

Reproduced identically on a clean committed tree — pre-existing, and an
**upstream bug in bevy_flair**, root-caused as:

- `StyleSystem`'s `#[default]` state is `Reset`, so every freshly spawned
  styled widget sits in `Reset` until the style pipeline consumes it (the
  same state is set when an entity's effective stylesheet resolves).
- flair's `mark_entities_for_recalculation` calls `recalculate_style()` on
  changed entities **and their siblings / descendants / ancestors** (via
  `RecalculateOnChangeFlags` — CSS selectors make one node's change affect
  others), without checking their state.
- Spawning a widget subtree while a relative's style data changes in the
  same frame — an ordinary UI-construction frame, and exactly what the
  login UI build burst does en masse — sweeps a still-`Reset` entity into
  the recalculation, tripping the `debug_assert`. Timing-dependent, hence
  the flaky 3-of-4 reproduction. The `SimulatorFeatures` CAPS warning that
  always preceded it was a red herring (diagnostics raise no UI; the
  warning just prints in the same instant as the UI build).
- Release builds were **silently wrong** rather than fine: the
  `debug_assert` compiles out and the pending `Reset` was downgraded to
  `CalculateStyle`, skipping the animation/transition reset.

Fixed per [[sl-client-fork-upstream-for-upstream-bugs]]: fork
`github.com/taladar/bevy_flair`, branch `fix-recalculate-style-on-reset` —
`recalculate_style()` keeps a pending `Reset` (which already implies a full
recalculation, mirroring the early-return the two other marking setters
already have for `Reset`), with a regression test and changelog entry.
Submitted upstream as <https://github.com/eckz/bevy_flair/pull/56>; the
workspace `[patch.crates-io]` pins all five flair crates to the fork rev
until a release carries the fix — then drop the patch.

Verified: three debug-build logins with the patch show zero panics where
the crash previously reproduced, and short screenshot runs complete the
full login → render (styled UI intact) → capture → clean-logout cycle.
