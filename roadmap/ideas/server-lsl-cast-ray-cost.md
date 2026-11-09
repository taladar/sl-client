---
id: server-lsl-cast-ray-cost
title: Charge llCastRay what each grid charges
topic: server
status: ideas
origin: test-lsl-differential-corpus-triage (2026-11-09)
refs: [test-lsl-differential-corpus-triage, server-world-collision-and-physics]
---

Context: [context/lsl.md](../context/lsl.md).

`throttle/cast-ray` measures how many rays a script casts a second: aditi
about 1 090 cheap and 45 expensive ones, the local OpenSim 12 to 22 of either
(its `m_msThrottleInCastRay` and the settings beside it), the fake grid about
5 000 on both flavours, since it charges a ray nothing. The rates are measured
lines, so the corpus agrees without this; a script that leans on raycasts
still runs far faster on the fake grid than on either grid. Model each grid's
cost: Second Life's per-region budget, which sets `RCERR_CAST_TIME_EXCEEDED`,
and OpenSim's throttle.
