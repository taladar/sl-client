# LSL topic — a full LSL implementation on the fake grid

Status: **committed**. Unlike the rest of [context/server.md](server.md),
which sizes a hypothetical real grid, this programme has a concrete
target: `sl-fake-grid` runs LSL scripts, so scripted content behaves
offline, deterministically, with no OpenSim and no network.

That target is worth stating precisely, because "run LSL" hides most of
the work. A script is not a function the grid calls; it is an
event-driven state machine that reads and writes an authoritative world.
The fake grid today has **no authority to read or write** — no tick, no
agent position, no chat routing, no touch. Roughly half of the tasks in
this topic are that missing simulator, not the language.

## Why, beyond "the fixtures should behave"

Two uses, and the second is the more valuable one.

The first is internal: scripted content is what makes a test grid a
*grid*, and nine conformance cases are live-grid-only today purely
because the fake grid cannot run a script
([[test-fake-grid-lsl-offline-cases]]).

The second is that an offline, deterministic, in-process grid that runs
a given set of scripts and asserts what they do is **a regression-test
harness for Second Life content** — a thing that does not really exist.
Today the only way to test a scripted product is to rez it on a live
region and click it. [[test-lsl-content-suite]] is that deliverable, and
taking it seriously changes the priority order of the library tranches:
what real content calls most matters more than completeness for its own
sake.

## What the audit found (2026-09-20)

Three layers already exist and are in good shape:

- **The language front end.** `sl-lsl` is a `logos` lexer, an
  error-tolerant recursive-descent parser over a fully-spanned AST, a
  semantic pass held to a no-false-positive bar, and `rustc`-grade
  diagnostic rendering. `sl-lsl-lsp` puts it behind LSP.
  `sl-lsl/tests/differential.rs` already diffs the semantic pass against
  **tailslide** over a corpus. What is missing is everything after the
  tree: no value model, no lowering, no interpreter.
- **The wire surface.** The LLUDP codec is generated from
  `message_template.msg` in both directions, and `SimSession` already
  carries most of what a script engine needs to *say*:
  `send_script_question`, `send_script_control_change`,
  `send_script_running_reply`, `send_chat_from_simulator`,
  `send_instant_message`, `send_avatar_animation`, `send_object_animation`,
  `send_sound_trigger`, `send_attached_sound`,
  `send_set_follow_cam_properties`, `send_avatar_sit_response`,
  `send_object_update`, `send_terse_update`. The gaps are narrow and
  enumerated in [[protocol-sim-script-messages]].
- **The viewer.** Script dialogs, permission questions, the contents
  Running/Reset checkbox, the script editor with a save-is-compile
  round-trip, hover text, particles and touch are all implemented client
  side. Nothing in this topic is blocked on the viewer; the viewer is the
  observer that makes the work checkable.

What is missing is the middle:

- `SceneFixtures` is one `parking_lot::Mutex<…>` over `Vec<Object>` with
  linear scans and no link-set structure beyond `parent_id`. It is a
  fixture store, not a scene graph.
- There is **no simulation loop**. A change is published by the session
  that made it and streamed to the region's other sessions; nothing
  sweeps, nothing ticks, nothing moves on its own.
- The agent's position is "the position the session was opened at"
  (`driver.rs`'s `stands_on`). `ServerEvent::AgentUpdate` is never
  consumed.
- `ServerEvent::Chat` is never consumed either: the fake grid hears local
  chat and drops it.
- `ObjectGrab` / `ObjectGrabUpdate` / `ObjectDeGrab`, `ScriptDialogReply`
  and `MoneyTransferRequest` are not decoded by `SimSession` — they fall
  through to the raw `ServerEvent::ClientMessage`. So a touch cannot
  reach a script.
- A script upload is accepted and the completion answers
  `compiled: Some(true)` unconditionally, with the comment "a real grid
  would run the compiler".
- The `LSLSyntax` capability serves whatever `SimSession::lsl_syntax()`
  holds, and the fake grid never sets it — an empty document.

## The five layers

1. **Values and semantics** — LSL's seven types with their exact
   arithmetic, coercion and formatting rules
   ([[server-lsl-value-model]]).
2. **Execution** — lowering the AST ([[server-lsl-compiler-ir]]), the VM
   that runs it in slices ([[server-lsl-vm-execution]]), the event and
   state machine ([[server-lsl-state-and-events]]), memory limits
   ([[server-lsl-memory-and-limits]]) and runtime error reporting
   ([[server-lsl-runtime-errors]]).
3. **The library** — ~425 `ll*` functions and 35 events, split into
   tranches by what they touch, tracked by a coverage harness
   ([[server-lsl-library-surface-table]]).
4. **The simulator facilities** the library reads and writes — an ECS
   scene store ([[server-world-ecs-store]]), a heartbeat
   ([[server-world-heartbeat]]), agent movement
   ([[server-world-agent-movement]]), chat routing
   ([[server-world-chat-routing]]), touch ([[server-world-touch-and-grab]]),
   link sets ([[server-world-link-sets]]), collisions
   ([[server-world-collision-and-physics]]), sitting and attachments
   ([[server-world-sit-and-attach]]), and update batching
   ([[server-world-update-scheduling]]).
5. **Integration and proof** — wiring the engine into the grid
   ([[server-fake-grid-script-engine-wiring]]), a scripted scenario
   ([[server-fake-grid-scripted-scenario]]) with scripted avatars to
   answer it ([[server-fake-grid-scripted-avatars]]), an offline script
   corpus ([[test-lsl-script-corpus]]), and a differential run against
   the local OpenSim ([[test-lsl-differential-opensim]]).

   The avatar half is easy to forget and half the library depends on it:
   a permission question, a dialog button, a payment, a sit and a played
   animation all need something with a **circuit** on the other end, and
   the grid's NPC fixtures are avatar-shaped objects with no session
   behind them. A scenario that exercises those paths has to bring a
   second real client and say what it does.

## The determinism rule

`sl-fake-grid` mints the same identifiers twice from one seed, and every
timestamp it stamps comes from an injected clock (`crate::time::Now`)
rather than `Instant::now`, precisely so two runs of a scenario are
comparable. A script engine is the single largest threat to that: timers,
`llFrand`, `llGetUnixTime`, `llGetTime`, HTTP, and a scheduler that runs
"as much as fits this tick" all read something outside the seed.

So the rule for this whole topic: **a script's observable behaviour is a
function of the seed, the scenario and the client's own input, and of
nothing else.** Concretely — the tick is a fixed step driven by the
injected clock, never wall time; `llFrand` draws from the grid's seeded
minter; a per-tick execution budget is counted in *instructions*, never in
elapsed microseconds; and anything that genuinely cannot be made
deterministic (outbound HTTP) is off by default and injected. This is
written down once here and enforced by
[[server-world-determinism-contract]].

## Reference sources

All read-only, all already on this machine.

| Path | What it is good for |
| --- | --- |
| `~/devel/3rdparty/opensim/OpenSim/Region/ScriptEngine/Shared/Api/Implementation/LSL_Api.cs` | The function surface, 18.5k lines: what each `ll*` does to a scene |
| `…/ScriptEngine/Shared/Instance/ScriptInstance.cs` | The event queue, run state, sleep, state change, XML state persistence |
| `…/ScriptEngine/XEngine/XEngine.cs`, `YEngine/` | Two scheduler designs: C#-compiled-per-script (X) vs a bytecode VM with continuations (Y) |
| `…/ScriptEngine/Shared/Api/Implementation/Plugins/` | `Dataserver`, `HttpRequest`, `Listener`, `ScriptTimer`, `SensorRepeat`, `XmlRequest` — the async command plumbing |
| `…/ScriptEngine/Shared/LSL_Types.cs` | The value model, including list and typecast behaviour |
| `~/devel/3rdparty/opensim/bin/ScriptSyntax.xml` | A complete `LSLSyntax` document (425 `ll*`, 224 `os*`, 35 events) |
| `~/devel/3rdparty/tailslide/builtins.txt` | A machine-readable signature table for the whole library |
| `~/devel/3rdparty/LSL-PyOptimizer/lslopt/lslbasefuncs.py`, `lsljson.py` | A precise, tested implementation of the **pure** library (strings, lists, math, base64, JSON) with SL's edge cases — the best oracle for the pure tranches |
| `~/devel/3rdparty/lslint` | A third front end to cross-check the parser against |
| `~/devel/3rdparty/phoenix-firestorm/indra/newview/llpreviewscript.cpp`, `llfloaterscriptdebug.cpp` | What the viewer expects to see of compile and run-time errors |

The local OpenSim (`opensim.service`, see the `sl-client` skill) is the
**behavioural** oracle: a script can be run there and on the fake grid and
the two observable streams diffed
([[test-lsl-differential-opensim]]).

## Non-goals

- **OSSL parity.** The `os*` surface is OpenSim-specific and mostly
  god-level; it gets one scoping task ([[server-lsl-lib-ossl]]) and no
  commitment.
- **Bytecode compatibility.** Nothing here reads or writes LSO bytecode or
  Mono assemblies. Scripts are compiled from source every time; the asset
  is the source, as it is on the wire.
- **Running other residents' content at scale.** The target is a test
  grid: correct semantics for scripts we write, not a hardened sandbox for
  hostile ones. Memory and instruction limits exist because scripts
  *observe* them ([[server-lsl-memory-and-limits]]), not as a security
  boundary.
