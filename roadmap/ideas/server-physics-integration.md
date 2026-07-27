---
id: server-physics-integration
title: Simulator physics integration
topic: server
status: ideas
origin: user request (2026-07) — size what a real server would involve
refs: [server-simulator-core]
---

Context: [context/server.md](../context/server.md).

Server-side physics for the simulator: avatar movement (the capsule
controller answering the client's `AgentUpdate` control flags — walk,
run, jump, fly, collisions with terrain/prims), physical objects
(rigid-body dynamics for `PrimFlags::Physics` objects), vehicles (the
LSL vehicle parameter model), collision events into the script engine,
and region-crossing ballistics.

Pluggable engine behind a trait, like OpenSim's ubODE/BulletSim split —
a pure-Rust engine (rapier, or jolt bindings) is the natural candidate.
The prim/mesh → collision-shape pipeline can reuse the client
workspace's mesh decoding; convex decomposition for mesh physics is the
expensive corner.
