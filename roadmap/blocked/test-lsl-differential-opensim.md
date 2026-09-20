---
id: test-lsl-differential-opensim
title: Run one script on both grids and diff what it did
topic: test
status: blocked
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-fake-grid-script-engine-wiring]
refs: [test-lsl-script-corpus, test-firestorm-crosscheck-runner,
  server-lsl-lib-math-rotations]
---

Context: [context/lsl.md](../context/lsl.md).

[[test-lsl-script-corpus]] pins what we *decided* the semantics are. It
cannot catch the case where we decided wrongly — where the expectation
and the implementation share one misunderstanding. For that the oracle
has to be a real simulator.

Two already exist and both are reachable from here: the local OpenSim
(`opensim.service`, a standalone region a script can be loaded into by
OAR merge or by `RezScript` over a live session) and, for the
Second-Life-only surfaces, aditi. And there is a precedent for the
shape: `sl-crosscheck` runs two *viewers* against one grid with one
capture block and collects what each wrote, deliberately collecting
rather than comparing, with `sl-crosscheck-report` doing the comparison
on named subjects. This is the same pattern rotated: one script, two
grids, one transcript comparison.

Wanted:

- a runner that, given a `.lsl` source and a small scene description,
  plants the script on each grid, drives the same stimulus (a touch, a
  chat line, a timer expiry, N ticks), and collects the **observable
  stream** — `ChatFromSimulator` in order, object updates, dialogs,
  animations — through an `sl-client-tokio` session, which is the only
  observer both grids share;
- a comparison that knows what may legitimately differ: minted ids,
  timestamps, float formatting at the last digit, update *coalescing*
  (a real simulator batches differently), and the ordering of things
  that genuinely have no defined order. Everything else is a finding;
- a small suite of scripts chosen to bite, starting with the ones a unit
  test cannot check honestly: rotation helpers
  ([[server-lsl-lib-math-rotations]] — an axis-order mistake produces
  plausible numbers), list functions with negative indices, the
  `llSetPos` 10 m clamp, event ordering on a state change, and the
  sensor cone's result ordering;
- and a record, the way the conformance harness records: committed,
  git-stamped, so a divergence has a history.

The local OpenSim is a **throwaway** grid the skill explicitly permits
reconfiguring, so a run may create accounts, load an OAR and restart the
region.

Acceptance: one command runs a named script on both grids and prints a
diff of the two transcripts; at least one real divergence found and
either fixed or recorded as a deliberate difference; the suite runnable
unattended against the local grid.
