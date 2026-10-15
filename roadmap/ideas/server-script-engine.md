---
id: server-script-engine
title: LSL script engine
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [server-simulator-core]
---

Context: [context/server.md](../context/server.md).

Running the scripts that make content behave: an LSL compiler +
runtime (event-driven state machines, per-script memory limits,
scheduling/throttling across thousands of scripts), the ll* function
surface (world queries/mutations into the scene, chat/IM, http-out,
listens, timers, sensors, permissions requests, animation/attachment
control), script state persistence (running state survives region
restart and travels with attachments), and the sandboxing/energy model.

The largest single work item on the simulator side after the scene
itself. OpenSim's XEngine/YEngine are the reference implementations;
compiling LSL to a WASM or bytecode VM for isolation is the design
question to settle first. The client workspace's `sl-wire` LSL syntax
support ties the compiler's surface to what the viewer's editor knows.

Audit 2026-10-15: the execution-model question is settled for the fake
grid: `sl-lsl-runtime` compiles LSL to a stack bytecode run by a VM in
per-tick instruction slices ([[server-lsl-architecture]],
[[server-lsl-compiler-ir]], [[server-lsl-vm-execution]]), and the rest of
this note's list is broken down as the `server-lsl-*` / `server-world-*`
tasks ([context/lsl.md](../context/lsl.md)). What stays here is the
real-grid part: thousands of scripts per region, sandboxing across hosts,
and state that travels with attachments between simulators.
