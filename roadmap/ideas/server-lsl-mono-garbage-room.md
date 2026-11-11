---
id: server-lsl-mono-garbage-room
title: Under a lowered limit, Mono has the room its reading's garbage gave it
topic: server
status: ideas
origin: server-lsl-mono-collision-model-under-lowered-limit (2026-11-11)
refs: [server-lsl-mono-collision-model-under-lowered-limit]
---

Context: [context/lsl.md](../context/lsl.md).

## Observation

Asked for `llSetMemoryLimit(used + N)`, two aditi scripts got further than
the runtime does:

| script | aditi | runtime | room it implies |
| --- | --- | --- | --- |
| `memory/recursion`'s Limited (`+ 4096`) | depth 129 to 253 | 118 | about 1 000 bytes |
| `memory/collision`'s Limited list (`+ 8192`) | 540 elements | 500 | about 640 bytes |

At the default limit the same scripts' twins match, and so does
`memory/stack`'s Quiet limited (672 against 560 to 928), which calls the
library every sixteenth level and makes next to no garbage. So the frame and
call context costs are not the gap (`vm/memory.rs`).

## Likely cause

aditi's `used` carries garbage, wandering by a few hundred bytes between two
reads of nothing. A limit set from a reading that held garbage has that
garbage as room once it is collected. The runtime has no garbage, so it gets
exactly `N`.

## Wanted

A deterministic model of the garbage a Mono reading carries, measured first:
a probe that reads `used` before and after making and dropping strings of
known sizes, and whether a lowered limit or a library call collects them. It
must not cut the default-limit depths, which garbage does not (Litter). Until
then the two rows above are the model's known lean, in their tests'
ranges.
