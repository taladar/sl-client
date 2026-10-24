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
| `drive` | `none` (runs from `state_entry`), or the stimuli the probe asks for with `waiting` records, comma-separated: `touch`, `chat`, `answer`, `sit`, `stand`, `control`, `dialog`, `away`; required |
| `script` | the inventory name, which the probe's scripts find each other by; required in a probe of several |
| `order` | where the script comes in the save order; required in a probe of several |
| `prim` | the object that holds it, `A` (the default), `B`, … |
| `distance` | how far east of prim A its prim stands, in metres |
| `height` | how far above prim A its prim stands, in metres — for a range no parcel is wide enough for (a chat range is a distance in three dimensions) |
| `skip-on` | the grids it is left out on, comma-separated, where it would take the region down instead of measuring anything — the comment says why |
| `vary` | the fields that are measurements (a duration, a rate, a clock), comma-separated: a comparison sets them side by side and never counts them as a difference |
| `unordered` | the records whose order among a run of their own kind is not defined, comma-separated: a comparison takes such a run as a set |

A probe that needs something done to it prints a `waiting` record and
waits; the runner answers each one, a second after it arrives, in order:

```text
PROBE events/other-avatar rec=waiting drive=chat channel=7 text="from the owner"
PROBE events/other-avatar rec=waiting drive=chat by=other channel=8 text="hi"
PROBE events/other-avatar rec=waiting drive=touch by=other
```

`drive=touch` touches prim A; `drive=chat` says `text` on `channel` at
normal range (20 m). `drive=answer grant=<mask>` answers the permission
question the probe's script puts next (`llRequestPermissions`) with that
mask as it is — `0` denies, and a mask beyond the question is sent too —
or notes that no question came within ten seconds. `drive=sit` sits on
prim A and `drive=stand` stands up. `drive=control keys=<mask>` holds the
movement controls `keys` (the `CONTROL_*` bits, which are the agent's own
control flags) down for a second, then lets go. `drive=dialog reply="…"`
replies to the newest dialog (`llDialog`, `llTextBox`) the probe put to
that avatar — the button with that label, which need not be one of the
dialog's (index 0 then), or the text typed into a text box — or notes that
none came within ten seconds; an older dialog the probe put without asking
for a reply is left unanswered. A reply is cut to the 254 bytes
`ScriptDialogReply` carries, as the reference viewer cuts it.
`drive=away seconds=<n>` teleports the owner out of the region (to a
neighbour on OpenSim; on aditi, where Mauve has none on the map, to the
first of the fixtures' `ban_line_regions` the map knows), keeps it there
`n` seconds, brings it back where it stood and walks it to the build
location again; it is the owner's alone, and the probe should hold back
its own lines meanwhile, since `llOwnerSay` may not reach an owner away.
`by=other` has the runner's
second avatar do it, which owns nothing of the probe's; without `by` it is
the owner. A probe asks only for the drives its header declares, and the
run fails if it asks for another.

```text
PROBE avatar/permissions rec=waiting drive=answer grant=1044
PROBE avatar/sit rec=waiting drive=sit by=other
PROBE avatar/controls rec=waiting drive=control keys=256
PROBE comms/dialog rec=waiting drive=dialog by=other reply="No"
PROBE comms/owner-away rec=waiting drive=away seconds=20
```

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

`--probe <id>` (repeatable) runs only the probes it names and writes only
their files:

```sh
sl-conformance run --grid opensim --probe events/other-avatar lsl-probes
```

The cases log in two avatars: the primary owns and runs the probes, the
secondary is the other avatar a `by=other` stimulus comes from and does
nothing else. On Second Life both walk from wherever the login put them
(Mauve's telehub) to the build location, so the second stands within chat
range of the prims.

For each probe, in id order, the case rezzes a cube per prim against the
region's reference primitive (prim A 1 m above it, the others `distance`
east). It makes each script in its prim under its name with `RezScript`,
in save order, and uploads the source running, prefixed on OpenSim with
the line that picks the engine (`//YEngine:` or `//XEngine:`). It answers
each `waiting` record and notes what it did (`# touch`, `# chat by=other
channel=7 text="…"`; a touch by the other avatar also says whether that
avatar has an active group, `active_group=set|none`, which decides what
`llDetectedGroup` answers and no script can read). It keeps the probe's
owner-say lines from its prims, and every `DEBUG_CHANNEL` message they send
as a `# debug-channel:` note (an owner-say line that is not a probe line as
`# owner-say:`), until `end` or two minutes, then derezzes the prims to the
Trash — and so it does when the grid refuses one of a probe's rezzes, with
the prims already rezzed, before the run fails. It also notes what the grid
sent the two viewers meanwhile, about the avatars: a permission question
from the probe's prims (`# question`, the mask, the object's name, how the
owner's name was put), a dialog from them
(`# dialog`, the message, the buttons in the order sent, the channel) and
the reply (`# dialog-reply`, the index and how many bytes went), an
instant message from an object (`# im`, the dialog, who the grid names as
sender and what the message id is, as roles — `owner`, `other`, `probe`),
any other line of the prims' chat an avatar hears (`# chat`, its type and
length, `# owner-say:` for the owner's own), the sit response
(`# sit`, seat position and rotation, camera offsets), a
`ScriptControlChange` (`# script-control-change`), a follow camera
(`# follow-cam`, the parameters by code, or `# follow-cam-clear`), an
avatar's animation list when it changes (`# animations`, the built-in ones
by name, each with `@probe` when the probe's prim is its source; an uploaded
one is the account's own and left out) and alerts. A long text is quoted to
its first 60 characters, with its length beside it. A trip `away` is noted
as `# away` and `# back`, with how each teleport went. Names are never written:
both avatars' names in anything a grid wrote become `<first>`, `<last>`,
`<other-first>`, `<other-last>`. Every wait inside a run keeps what arrives
during it, so a line printed while the case waits for a compile or a
listing is not lost.

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

The case collects; it never compares.

## Comparing

`sl-conformance compare-probes` sets the committed result files side by
side, grid against grid, with no login:

```sh
# every probe, aditi against each OpenSim engine
sl-conformance compare-probes
# one probe, two grids, the first the baseline
sl-conformance compare-probes --grid opensim-yengine --grid opensim-xengine \
  events/queue-limit
```

For each probe the first grid that has a result is the baseline, compared
with each other grid that has one. The comparison
(`sl_lsl_runtime::probe::compare`) knows what may legitimately differ:

- **minted ids**: any key but `NULL_KEY`, anywhere in a value, reads as one
  placeholder;
- **a float's last digit**: two decimals agree within one unit of the last
  place the coarser prints (`5.0` and `5.000000`, `0.123456` and
  `0.123457`); an integer has to match; vectors and rotations component by
  component;
- **measurements**: the header's `vary` fields are reported as `measured`,
  never as a difference;
- **context**: the `start` record's region, channel and version;
- **order**: the lines before `start`, and a run of an `unordered` record,
  are compared as sets.

Records are matched by the longest common sequence of records that agree;
between two matched ones, a left and a right record of the same name are
one record whose fields differ, and the rest were printed by one run only.
The `#` notes are not compared. What is left is a finding:

```text
== events/queue-limit: aditi | opensim-xengine — 2 difference(s)
  differs     received sent=100 lowest=0: got 64 | 100
  differs     received sent=100 lowest=0: highest 63 | 99
== vm/throughput: aditi | opensim-yengine — agree
  measured    loop loop=empty run=0 iterations=1000000: per_second 505586 | 27777779
```

Each row names the record with the fields both runs agree on, then the
field and the two values. Nothing yet needs a tolerance for merging (a
grid coalescing what another sends apart): that is the frame timing no
probe should depend on (above).

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
| `avatar/*`, 2026-10-24: permissions, sitting, controls, camera and animations | read in [Grid Behaviour → LSL scripts](../gridspec/lsl.md#animations-sitting-controls-and-camera) | | |
| `comms/*`, 2026-10-24: listens, object chat, dialogs, text boxes, instant messages, the owner elsewhere, link messages | read in [Grid Behaviour → LSL scripts](../gridspec/lsl.md#chat-listens-dialogs-and-link-messages) | | |
| `events/other-avatar`, 2026-10-24: a listener on 7 | hears the owner and the other avatar, `key_is_owner` 1 and 0, the name `llKey2Name`'s | the same | the same |
| — a listener on 8 filtered to the owner | does not hear the other avatar | the same | the same |
| — the other avatar's touch | `llDetectedType` 3 (`AGENT \| ACTIVE`); `llDetectedGroup` 1, the object's group and the avatar's active group both unset | 1 (`AGENT`); `llDetectedGroup` 0, the avatar's active group set (the object's group compared with the toucher's active group, as OpenSim's source reads) | the same as YEngine |
