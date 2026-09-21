---
id: server-lsl-architecture
title: LSL engine architecture — execution model, crate layout, scheduling
topic: server
status: ready
origin: LSL-on-the-fake-grid audit (2026-09-20)
points: 5
refs: [server-script-engine, server-lsl-value-model, server-lsl-compiler-ir,
  server-world-heartbeat]
---

Context: [context/lsl.md](../context/lsl.md).

The umbrella decision task. Everything in the LSL topic rests on four
choices that are cheap to make now and expensive to change once a
thousand library functions are written against them. Make them, write
them down here and in the crate's `README.md`, and stop.

**1. Where the code lives.** `sl-lsl` is deliberately I/O-free and
Bevy-free and is a *client* dependency (the editor, the LSP server, the
highlighter) — a runtime that reaches into a scene does not belong in it.
The proposal is a new workspace crate **`sl-lsl-runtime`**: value model,
lowering, VM and the library *dispatch table*, with every world-touching
library function expressed against a **`Host` trait** the crate defines
and does not implement. `sl-fake-grid` implements `Host` over its region
world. That keeps the runtime unit-testable with a mock host (which is
what [[test-lsl-script-corpus]] needs) and keeps `sl-lsl` clean. Decide
also whether the value model lands in `sl-lsl` (shared with the semantic
pass, which already reasons about types) or in the runtime.

**2. The execution model.** Three candidates, in rising cost:

- a **tree-walking interpreter** over the resolved AST, with an explicit
  continuation so a script can be suspended mid-statement;
- a **register or stack bytecode** compiled per script, like YEngine's
  `MMRScript*`;
- compiling to a host language, like XEngine's C# emission — not an
  option here.

The hard constraint is not speed, it is **suspension**: `llSleep`, a
state change, an execution budget and `llResetScript` all have to stop a
script *between* instructions and resume it later, and the fake grid runs
one region on one tick. A tree walker can do this only with an explicit
machine (a Rust `async` generator or a hand-rolled continuation); a
bytecode VM gets it for free because its program counter is a number. The
recommendation is **bytecode**, and the task is to confirm it against
`YEngine/MMRScriptCodeGen.cs` and record why.

**3. Scheduling.** One region, one tick ([[server-world-heartbeat]]),
N scripts. Decide: a fixed **instruction budget per script per tick**
(deterministic, and what [[server-world-determinism-contract]] requires),
round-robin over runnable instances, with a script that exhausts its
budget resuming next tick. Never a wall-clock slice. Decide what happens
when the region is over budget — every script simply runs slower, as a
real simulator does, and `llGetRegionTimeDilation` reports it.

**4. The library dispatch shape.** ~425 functions. Decide the signature
once: a function is `fn(&mut Vm, &mut dyn Host, &[Value]) -> Result<Value,
RuntimeError>` plus a static descriptor (name, argument types, return
type, energy, sleep) generated from one table, so
[[server-lsl-library-surface-table]] can check coverage and
[[protocol-sim-lsl-syntax-document]] can render the grid's `LSLSyntax`
document out of the same table rather than a second hand-kept list.

Acceptance: a short design chapter in `book/src/` (or the new crate's
README) stating the four decisions and the reason for each, plus the
`Host` trait sketched with the half-dozen methods the first tranche
needs. No implementation — the point is that the tranches after it do not
each re-litigate this.
