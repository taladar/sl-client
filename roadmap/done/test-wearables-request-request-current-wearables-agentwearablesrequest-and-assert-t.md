---
id: test-wearables-request
title: request current wearables (AgentWearablesRequest) and assert the simul
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 14 — Appearance, attachments & animations `[both]`
---

Context: [context/test.md](../context/test.md).

`wearables-request` — request current wearables (`AgentWearablesRequest`)
and assert the simulator's `AgentWearablesUpdate` reply
([`Event::AgentWearables`]: serial + the worn wearables). `1av`. The command
and event already existed; this is the first case to exercise them. **Grid
divergence.** On OpenSim the legacy message carries the real outfit, so the
case asserts all four mandatory body parts (shape / skin / hair / eyes) are
present, each worn exactly once and naming a real asset (green: 6 wearables =
4 body parts + 2 clothing, serial 15). On modern Second Life (aditi) the
outfit is managed server-side (central baking + the Current Outfit Folder over
AIS3) and `AgentWearablesUpdate` is *transitional/deprecated* — the simulator
answered with only 3 body parts (missing **Shape**) + 1 clothing, which the
reference viewer's `processAgentInitialWearablesUpdate` itself treats as a
dummy and ignores, reading the true outfit from the COF instead. So an
incomplete reply on aditi is recorded `partial` with the missing slots, not a
failure — the same grid-divergence shape as `server-appearance-bake`. The
modern SL-native way to read the outfit is `current-outfit-folder` below.
`[both]`.
