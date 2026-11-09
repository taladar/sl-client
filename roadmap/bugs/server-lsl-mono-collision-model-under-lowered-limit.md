---
id: server-lsl-mono-collision-model-under-lowered-limit
title: A script 1 KB over its use collides in the runtime where Mono carries on
topic: server
status: bugs
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

`memory/budget` sets `llSetMemoryLimit(llGetUsedMemory() + 1024)` and goes on
calling a helper that builds a line of about 90 characters. aditi carries on
with 812 to 962 bytes free, its use rising some 150 to 210 bytes while the line
is built. The runtime's Second Life flavour raises a stack-heap collision in
the next call, so on fake-sl the probe never prints its last six `set-limit`
lines or its `end`.

## Cause

The collision model (`sl-lsl-runtime/src/vm/memory.rs`: `MONO_FRAME`,
`slot_size`, and the locals' heap objects through `local_cost`) charges the
call chain `state_entry → ask_limit → read → emit` and its temporaries more
than 1 KB. `memory/recursion`'s Limited script shows the same lean: it reaches
depth 73 where aditi reaches 129 to 253.

## Fix

Fit the model to this probe and to Limited as well, keeping the fits at the
default limit (`vm/tests/memory.rs`). Then replace `memory/budget`'s
whole-probe entry in `probes/accepted.toml` with field entries for the code
model's offset (`read`, `held`, `text`, `profiler`, `set-limit` × `used`,
`free`).
