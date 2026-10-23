# The LSL probe corpus

An LSL question — how many events a queue holds, what a run-time error
says, how fast a loop runs — is settled by asking each grid, not the
documentation. The probe corpus is the committed set of scripts that ask,
and the result files are what each grid answered:

- `sl-lsl-runtime/probes/<area>/<probe>.lsl` — a probe of one script, or
  `sl-lsl-runtime/probes/<area>/<probe>/*.lsl` — a probe of several, which
  run together in one or more prims;
- `sl-lsl-runtime/probes/results/<grid>/<area>/<probe>.txt` — one run's
  transcript, for each of `aditi` (Second Life's Mono), `opensim-yengine`,
  `opensim-xengine` and, later, `fake-sl` and `fake-opensim`.

The format lives in `sl_lsl_runtime::probe`; the test
`sl-lsl-runtime/tests/probe_corpus.rs` compiles every probe and holds every
result file to it.

## Writing a probe

A probe's id is its path below `probes/` without the extension
(`events/queue-limit`). Every script opens with a header of comment lines:

```lsl
// probe: errors/runtime-errors
// script: Listener far
// prim: B
// order: 5
// distance: 50
// drive: none
// skip-on: opensim-xengine
```

| key | meaning |
| --- | --- |
| `probe` | the id; required |
| `drive` | `none` (runs from `state_entry`) or `touch` (prints a `waiting` record, then waits for one touch of prim A); required |
| `script` | the inventory name, which the probe's scripts find each other by; required in a probe of several |
| `order` | where the script comes in the save order; required in a probe of several |
| `prim` | the object that holds it, `A` (the default), `B`, … |
| `distance` | how far east of prim A its prim stands, in metres |
| `skip-on` | the grids it is left out on, comma-separated, where it would take the region down instead of measuring anything — the comment says why |

What a probe prints is one `llOwnerSay` line per observation:

```text
PROBE <id> rec=<record> key=value key="quoted value" …
```

`rec` comes first. A value is a bare token, or a `"…"` string with `\\`,
`\"` and `\n` escapes — the `q()` helper every probe carries prints that.
The probe's leading script prints one `rec=start` (region, `sim_channel`,
`sim_version`) and, when it is done, one `rec=end`, last. Scripts saved
before the leader may print first. Never print an avatar's name: print
whether a key is the owner's (`key_is_owner=1`) instead, because results
are committed.

What has bitten already, and how a probe should be shaped for it:

- **`llGetTime` moves in whole frames on Second Life** (1/45 s, measured by
  `time/get-time-frames`). A loop timed over 50 000 iterations is off by a
  frame, about a quarter. Time a million, as `vm/throughput` does.
- **The first run after a save is slower** while Mono compiles. Run the
  measurement twice and read the second (`run=1`).
- **Keep a probe in one state** and toggle a global instead. A `state`
  statement discards the queue, so a `state x;` at the end of a handler
  throws away everything the probe just queued (`events/state-change`).
- **Do not queue 64 events before a timer you want to see.** Second Life's
  and YEngine's queue holds 64, and a full queue drops the timer
  (`events/queue-limit`). Set the timer once the queue has drained.
- **Merging is timing.** Touches, collisions and `changed` merge on the
  grid by frame timing no script controls. A probe that depends on how
  they merge measures the frame, not the rule.
- **A function name is not free.** `control`, `timer` and the other event
  names cannot name a function.
- **Some failures take the region with them.** Unbounded recursion under
  XEngine overflows the simulator's own stack, and the whole OpenSim
  process aborts. That is what `skip-on` is for.

## Running

`sl-conformance` runs the whole corpus on a live grid in one login and
writes the result files:

```sh
# Second Life, Mono
sl-conformance run --grid aditi lsl-probes
# OpenSim, YEngine
sl-conformance run --grid opensim lsl-probes
# OpenSim, XEngine
sl-conformance run --grid opensim lsl-probes-xengine
```

For each probe, in id order, the case rezzes a cube per prim against the
region's reference primitive (prim A 1 m above it, the others `distance`
east). It makes each script in its prim under its name with `RezScript`,
in save order, and uploads the source running, prefixed on OpenSim with
the line that picks the engine (`//YEngine:` or `//XEngine:`). It touches
prim A a second after a `touch` probe says it is waiting. It keeps the
probe's owner-say lines from its prims, and every `DEBUG_CHANNEL` message
they send as a `# debug-channel:` note, until `end` or two minutes, then
derezzes the prims to the Trash.

A result file's header says where and how the run was made:

```text
grid: aditi
date: 2026-10-23
runner: sl-conformance lsl-probes (rezzed, uploaded and read by our client)
region: Mauve
engine: Mono
settings: Linden Lab's; a resident cannot set them

PROBE events/queue-limit rec=start region="Mauve" channel="Luau" …
PROBE events/queue-limit rec=received sent=100 got=64 lowest=0 highest=63
PROBE events/queue-limit rec=end
```

`settings` on OpenSim is the `lsl_settings` fixture (`fixtures.toml`): the
engine settings the operator configured (`MinTimerInterval`,
`ScriptDelayFactor`, the sensor limits), which no script can read. A run
that never printed `end` says `complete: no`, and the conformance record
is `partial`. On Second Life the case needs the fixtures' build location,
as every case that rezzes does.

The case collects; it never compares. Telling what may legitimately
differ — minted ids, timestamps, a float's last digit — from a finding is
the live differential runner's job (`gridspec-lsl-live-differential-runner`).

A probe can also be run by hand: drop its scripts into prims in `order`,
under their `script` names (prefixed on OpenSim with the engine line), and
copy the `PROBE` lines out of the chat.

## What the corpus has measured

The first runs, 2026-10-23: aditi's Mauve (`sim_channel` `Luau`, version
`2026-07-24.30110090421`, scripts compiled to Mono) and the local OpenSim
0.9.2.1 under each engine. The result files hold every number; this is
the reading. The grid tasks (`gridspec-lsl-events`, `-time-timers`,
`-runtime-errors`) take them from here into the grid chapters.

| probe | Second Life (Mono) | OpenSim YEngine | OpenSim XEngine |
| --- | --- | --- | --- |
| `vm/throughput`, a million iterations | 511 000/s empty, 500 000 with three operations, 228 000 calling `llAbs` | 30 million, 29 million, 11 million | 416 000, 438 000, 434 000 — flat, the call is free |
| `time/get-time-frames` | steps of one frame, 0.0219–0.0227 s | 1 ms steps | 1 ms steps |
| `events/queue-limit`, 100 link messages to itself | 64 arrive (0–63) | 64 arrive | all 100 arrive |
| `events/timer-no-stack`, 0.1 s timer over a 2 s sleep | runs once on waking, then every 0.09–0.11 s | once, then every 0.1 s | once, then every 0.5 s (`MinTimerInterval`) |
| `events/state-change` | queued messages dropped, timer kept, listen dropped | the same | the same |
| `events/state-self`: `state default` in `default` | runs neither `state_exit` nor `state_entry` | the same | the same |
| `events/state-self`: `state` in a function | the caller carries on; the change comes when the handler ends | the handler ends at the `state` | the same as YEngine |
| `events/detected-cleared`: `llDetected*` outside the touch | `NULL_KEY`, the name `NULL_KEY`'s text, face 0 | key and name empty, face −1 | the same as YEngine |
| — the touch itself | `llDetectedType` 3 (`AGENT \| ACTIVE`) | 1 (`AGENT`) | 1 |
| `time/sleep-while-stopped` | the sleep counts on while stopped: restarted after it would have ended, wakes at once; before, wakes at 5 s | the same | stopping ends the sleeping event: it never wakes |
| `errors/runtime-errors`: the message | two from the prim, heard by a script 10 m away and not by one 50 m away: `Object [script:<name>] Script run-time error`, then `Math Error` (integer and float division, modulo) or `Stack-Heap Collision` | one to the owner, which no script hears: `YEngine: Division by Zero (script: … primID: … at: <…>) Script must be Reset to re-enable.` | one to the owner, which no script hears: `Integer division by Zero(script: … at <…>)` |
| — a library function's error | `Could not find script 'Nope'.`, heard at 10 m, not at 50 m | `llGetScriptState: Can't find script 'Nope'`, heard at 50 m | the same |
| — the faulted script | stopped: `llGetScriptState` 0, a link message goes nowhere, and `llSetScriptState(…, TRUE)` does not start it again | reported running, yet answers nothing, restarted or not | runs on: answers in the state it faulted in, globals intact |
| — unbounded recursion | `Stack-Heap Collision` | a `StackOverflowException` the engine catches (logged; no message) | the simulator process aborts — `skip-on` |
