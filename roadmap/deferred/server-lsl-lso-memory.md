---
id: server-lsl-lso-memory
title: Run an LSO-targeted script in LSO's 16 KB, as Second Life does
topic: server
status: deferred
origin: server-lsl-memory-and-limits (2026-10-30)
points: 5
refs: [server-lsl-memory-and-limits, server-fake-grid-script-compile-on-upload]
---

Context: [context/lsl.md](../context/lsl.md).

`server-lsl-memory-and-limits` accounts every script as Mono's (or as the
OpenSim engine's), because the fake grid compiles every script from source
and keeps no target. Second Life still compiles a script uploaded with the
LSO target (`target: lsl2` in a probe header, the viewer's "Mono" checkbox
unticked) to LSO, and such a script reads its memory differently
(`book/src/gridspec/lsl.md` § Memory and limits): `llGetUsedMemory` is
always 16 384, `llGetMemoryLimit` 16 384, `llSetMemoryLimit` takes only
16 384, `llGetFreeMemory` is a low-water mark of a 16 KB budget, and it runs
out sooner — a doubled string at 8 192 characters, a one-integer recursion
at depth 763, every run.

Two things before it can be built:

- **The target has to reach the instance.** The upload's target is on the
  wire (`UpdateScriptTask`/`UpdateScriptAgent`'s `target`); the compile on
  upload ([[server-fake-grid-script-compile-on-upload]]) would keep it with
  the program, and the instance would account by it.
- **LSO's per-value costs are not measured exactly.** The memory probes saw
  LSO only through its low-water mark, which a list's growth moved unevenly
  ("about 30" an integer, the copy an append makes included; strings within
  an earlier mark). A `memory/sizes-lso` probe, reading `llGetFreeMemory`
  after each value from a freshly reset script, would give the exact costs
  the accounting needs; until then an LSO model would be a guess, which the
  memory task's rule ("exact, not approximated") forbids.

## From server-fake-grid-script-compile-on-upload (2026-10-30)

The compile on upload exists now (`sl_fake_grid::script_compile`), but it
keeps no program and so no target: an `lsl2` upload compiles exactly as a
`mono` one. Carrying the target (`UploadedScriptItem::target`) to the
instance belongs with the engine wiring
([[server-fake-grid-script-engine-wiring]]). The census's one compile answer
LSL2 gives differently is `large_code` — 600 functions of 100-character
strings, refused with 1203 errors of no position, where Mono compiles it —
and `script-compile`'s `KNOWN_DEVIATIONS` lists it until LSO's 16 KB is
modelled at compile time too.

## Deferred (2026-11-01): exact needs LSO itself

The LSO compiler and VM Linden Lab once shipped with the viewer are still in
Firestorm's history: `indra/lscript/`, removed by `ca08bd5aba` (OPEN-292), so
`git archive ca08bd5aba^ indra/lscript` in `~/devel/3rdparty/phoenix-firestorm`
recovers them. They settle the per-value question the probe above was for,
and the answer is that there are no per-value costs to measure:

- `llGetFreeMemory` is `SP - HP` (`LLScriptExecuteLSL2::getFreeMemory`): the
  stack pointer at the call less the heap's high-water mark. That is why
  `memory/budget-lso` reads 3 550 from `state_entry` and 3 530 from inside a
  helper with nothing allocated between: the helper's frame is on the stack.
- The heap (`lscript_alloc.cpp`, `lsa_heap_add_data`) is a chain of blocks
  with a 7-byte header (4-byte size, 1-byte type, 2-byte reference count),
  allocated first fit, a block split when 11 bytes or more are left over,
  freed blocks merged with an empty successor only during a later search.
  `HP` only rises. A list is a block of `4 + 4n` bytes followed by a block per
  element, so how far `HP` moves for `held += [n]` depends on where the old
  list and the temporaries sat — the uneven "about 30" an integer.
- The free memory a script starts with is 16 384 less its compiled image
  (`lscript_bytecode.cpp`): the LSO emitter's byte sizes
  (`lscript_tree.cpp`, the `LSCP_EMIT_BYTE_CODE` pass) over globals,
  functions and states. `large_code`'s refusal is that image overflowing.

So an exact model is a port of the LSO code emitter (sizes) and of the LSO
VM (`lscript_execute.cpp`, the heap, the stack frames) running an `lsl2`
script on the Second Life flavour, bridged to the existing library — which
also contradicts [context/lsl.md](../context/lsl.md)'s "no LSO bytecode"
non-goal. Deferred by the user's call rather than built as an
approximation.
