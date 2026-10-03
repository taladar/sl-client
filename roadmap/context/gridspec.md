# Gridspec topic — measure each live grid, document it, make the fake grid be it

The `gridspec-*` tasks pin down **exactly how each live grid behaves** —
Second Life (measured on the beta grid, aditi) and OpenSim (the local
`opensim.service`, both YEngine and XEngine for scripts) — so that
`sl-fake-grid` can reproduce the whole grid, per flavour, and our viewer can
be held to both. The survey that filed them is [[gridspec-survey]].

`sl-fake-grid` already names the grid it imitates once
(`FakeGridBuilder::imitates`, `sl-fake-grid/src/imitates.rs`) and takes every
divergent behaviour's default from that flavour. Each `gridspec-*` task grows
that table by one feature.

## What one task does

Every discovery task has the same four parts, and is done when all four are:

1. **Discover** — measure the feature on **aditi and OpenSim** with the
   automation we have, in this order of preference:
   - the `sl-conformance` cases (extend a case to *record the whole shape* —
     every field, the transport, the order — not only pass / fail), run with
     `--grid aditi` and `--grid opensim`;
   - `sl-repl --script` wire probes (`set_diagnostics on` dumps raw messages);
   - the viewer automation — `sl-e2e` stages with `SL_E2E_GRID=aditi|opensim`
     (one or two viewers), `sl-viewer-ctl` — for anything the viewer shows;
   - for LSL, the probe corpus and the live differential runner
     ([[gridspec-lsl-probe-corpus]], [[gridspec-lsl-live-differential-runner]]);
   - only what none of these can reach goes to the user: run the viewer (or
     Firestorm) for them and let them place the camera, press the button or
     take the screenshot. Do not build convoluted automation for that part.

   Aditi rules: never `--force` past the 120 s per-avatar login cooldown, but
   do not avoid aditi either; uploads cost real (beta) L$; rezzing leaves
   sandbox debris — clean up. Land, estate and terraform legs that need
   rights on aditi wait on [[gridspec-aditi-test-land]].

2. **Document** — write the measured behaviour into the book's *Grid
   Behaviour* part, `book/src/gridspec/<chapter>.md` (named in each task;
   several tasks share a chapter), as a table per behaviour with a column per
   grid (Second Life | OpenSim | what the fake grid's flavour does), each cell
   saying when and how it was measured (case, date). Unmeasurable legs say so
   and why. Link the chapter from the matching `content/` chapter.

3. **Fake grid** — a **small** gap is implemented in the same task: a row in
   `imitates.rs` per divergence, the behaviour per flavour, and the task's
   conformance cases added to the offline list for both `FakeSl` and
   `FakeOpensim` ([[gridspec-infra-fake-flavour-conformance]]) so the fake
   grid is held to the live answer. A **large** gap is a separate
   implementation task with `blocked_by:` the discovery task (named in the
   task's *Fake grid* section — an existing `server-*` / `server-lsl-*` task
   or a new one).

4. **Viewer** — check that our viewer handles **each** grid's behaviour: an
   `e2e` test against each fake flavour where possible, a live check
   otherwise. Fix small gaps in the task; file a bug for anything larger.

## Boundaries

- The `server` topic's committed LSL line ([context/lsl.md](lsl.md)) is
  unchanged; its `server-lsl-lib-*` / `server-world-*` tasks are now blocked
  by the matching `gridspec-lsl-*` / `gridspec-*` discovery, so they are built
  to the measured behaviour rather than to the documentation.
- Conformance records stay gitignored; the book tables are the committed
  record of what was measured.
