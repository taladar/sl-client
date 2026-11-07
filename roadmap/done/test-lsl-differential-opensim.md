---
id: test-lsl-differential-opensim
title: Run one script on both grids and diff what it did
topic: test
status: done
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 8
blocked_by: [server-fake-grid-script-engine-wiring,
  gridspec-lsl-live-differential-runner]
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

## From gridspec-lsl-live-differential-runner (2026-10-24)

The live half and the comparison exist: `sl-conformance run --probe <id>
lsl-probes` plants and drives one probe (touch, chat, a second avatar), and
`sl-conformance compare-probes` diffs result files under the tolerance
model in `sl_lsl_runtime::probe::compare` (ids, a float's last digit, `vary`
measurements, `unordered` records). Left here: the fake flavours' runs into
`probes/results/fake-sl|fake-opensim/`, and recording a divergence as a
deliberate difference.

## Done (2026-11-07)

- **The fake flavours in the probe runner.** `sl-conformance run --grid
  fake-sl|fake-opensim lsl-probes` runs the corpus on the in-process fake
  grid exactly as on a live one (rez, upload, drive, collect) and writes
  `probes/results/fake-sl/` and `probes/results/fake-opensim/`; a skip-on note
  names the grid the flavour imitates, and the estate owner's region restart
  falls back to the owner on a fake grid. The fake runs are runner-only in the
  offline registry (`fake::RUNNER_ONLY`): one corpus run is hours of region
  time. Both corpora were run in full (fake-sl's last probe,
  `vm/throughput`, ran past the three-hour timeout).
- **One command, one script, both grids.** `sl-conformance differential
  [--grid opensim|aditi] [probe…]` runs the probes on the live grid, then on
  its fake flavour, then prints `compare-probes` of the two. Without names it
  runs the differential suite (`probe_corpus::DIFFERENTIAL_SUITE`):
  math/rotations, math/functions, strings/functions, prim/placement,
  events/state-change, events/order, detection/sensor, throttle/delays,
  throttle/chat. Against the local OpenSim it runs unattended (about thirteen
  minutes) and every probe of the suite agrees, or agrees but for accepted
  differences.
- **Deliberate differences and known defects on record.**
  `probes/accepted.toml` (`sl_conformance::accepted`): an entry names the
  probe, the two result sets, the record, the fields that pick it out and the
  field that differs, with a reason, and a `task` for a known defect; a test
  fails an entry that covers nothing any more or whose task is not open.
  `compare-probes` reports covered differences as `accepted`, apart.
- **Divergences found and fixed:**
  - OpenSim's `vector * rotation` and `vector / rotation` are its own formula
    (`v + 2 q×(q×v + s v)`, in double) rather than the quaternion sandwich:
    `ops::opensim_rotate` (math/rotations).
  - Second Life's ranked queue starved a `changed` and the touches behind each
    round's fence link messages (events/order: fake-sl printed neither, aditi
    both): the rank now orders only the events raised while one handler ran,
    each behind every event raised before that handler began
    (`Queued::batch`; `a_changed_waits_behind_its_own_handlers_events_alone`).
  - `llFrand`'s least and greatest draw are measurements: math/functions
    declares them `vary`.
- **Recorded as deliberate:** OpenSim's doubles (math/functions,
  math/rotations), Second Life's per-run `llBase64ToInteger` of junk, the
  rotation round trip three units in the last place off aditi's, the fake
  region's terrain and
  build height (prim/placement), the live regions' other objects and avatars
  and Second Life's scripted-prim drift (detection/sensor), OpenSim's 100 ms
  sensor loop racing the probe's timers, and the talker and the probe saved
  in one frame over loopback (events/state-change; the VM replay
  `a_state_change_probe_hears_what_each_grid_heard` hears six with the talker
  ahead, five in one frame).
- **Recorded as known defects:** [[server-lsl-event-raise-timing]] (OpenSim
  raises `changed` and `llSensor`'s answer inside the call, Second Life
  delivers chat before link messages) and [[server-lsl-sl-placement-lag]]
  (Second Life's late bounding box and its stop 4 cm above the ground).
- The runner puts the avatars' names as placeholders in a probe's own lines
  too, not only in its notes: on the fake grid `osGetAvatarList`-style
  answers printed them.
- The local OpenSim's build position had collected eight probe objects left by
  cancelled runs (still scripted, at distance 0 from every probe); they were
  deleted, and `tools/lsl-probes.md` says to look for them.

## Left open

- The rest of the corpus is compared but not triaged:
  [[test-lsl-differential-corpus-triage]] lists every probe that differs
  between each grid and its fake flavour.
