---
id: viewer-vintage-ui-chrome-crosscheck
title: Measure skin fidelity instead of arguing about it
topic: viewer
status: ready
origin: Vintage skin fidelity audit (2026-09-20)
points: 3
refs: [viewer-vintage-skin, test-firestorm-crosscheck-runner,
       test-firestorm-crosscheck-report, test-firestorm-harness-skin-selection]
---

Context: [context/viewer.md](../context/viewer.md),
[context/vintage-skin.md](../context/vintage-skin.md).

Every other task in this group is a claim about how two viewers look side by
side, and right now the only way to check one is to run both and squint.
`sl-crosscheck` already runs both viewers against an in-process fake grid with
one capture block, and `SL_VIEWER_CAPTURE_UI=1` already makes both capture
their chrome. Two things stand between that and a usable skin measurement.

**The reference run wears the wrong skin** — it comes up in Firestorm's
default skin every time, so a comparison against "the reference's Vintage"
measures nothing. That is its own task on the fork's harness,
[[test-firestorm-harness-skin-selection]], because the settings involved are
subtler than they look; this task only needs the `--skin` / `--theme` pair it
adds to `sl-crosscheck` to reach both viewers.

**A UI capture resizes the window, and only on that side.** The reference's
snapshot path cannot draw the UI at any size but the window's, so
`SL_VIEWER_CAPTURE_UI=1` makes its harness reshape the window to the capture
size and warn if the compositor declines; ours lays the UI out at the capture
size directly. A UI cross-check is therefore the one run where a tiling
compositor can silently make the two frames incomparable. The run should read
back what it got and say so, rather than producing a pair that differs by a
scale factor nobody notices.

Also worth stating plainly in the task's output, because it bounds what this
can measure: **the harness closes every floater and blocks them for the run**,
so a UI capture is a comparison of *chrome* — menu bar, toolbars, chat bar,
status row — and not of floater rendering. A notification popping between two
frames would otherwise make a sequence incomparable, which is why the block
exists and why it should stay.

## What to do

- `--skin` / `--theme` on `sl-crosscheck`, into the shared capture block, so
  one flag dresses both viewers ([[test-firestorm-harness-skin-selection]] is
  the Firestorm half).
- A recorded chrome-capture pair per skin under the cross-check's run
  directory, the way the scene dumps are recorded.
- Report the window-vs-capture size mismatch as a run failure, not a warning
  buried in a log — the existing `harness-status.json` is the place.

## Done when

`sl-crosscheck --skin vintage --capture-ui` produces a chrome pair from both
viewers at the same size, or fails saying why, and the pair is what the
Vintage-alike skin's fidelity is judged against.
