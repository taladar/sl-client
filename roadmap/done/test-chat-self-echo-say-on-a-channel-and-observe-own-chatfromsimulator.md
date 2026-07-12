---
id: test-chat-self-echo
title: say on a channel and observe own ChatFromSimulator
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 2 — Local chat `[both]`
---

Context: [context/test.md](../context/test.md).

`chat-self-echo` — `say` on a channel and observe own
`ChatFromSimulator`. `1av`, runs on Aditi today. A normal `say` on the
public channel (`0`) is broadcast back to the speaker, so the case sends a
marker message tagged with the avatar's own agent id, then awaits the
matching `Event::ChatReceived` attributed to its own agent — asserting the
echoed text, source, and `Normal` chat type. Green on both grids; echo RTT
≈ 18 ms on loopback OpenSim, ≈ 177 ms on Aditi.
