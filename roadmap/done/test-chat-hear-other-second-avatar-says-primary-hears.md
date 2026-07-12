---
id: test-chat-hear-other
title: second avatar says, primary hears
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 2 — Local chat `[both]`
---

Context: [context/test.md](../context/test.md).

`chat-hear-other` — second avatar says, primary hears. `2av`
(OpenSim now; Aditi deferred → Phase Z). The first multi-avatar case: the
secondary (`Friend Tester`) `say`s a marker tagged with its own agent id on
the public channel, and the primary (`Avatar Tester`) — a separate session
sharing the region — receives the matching `Event::ChatReceived` attributed to
the secondary's agent, `ChatAudible::Fully`, `Normal` volume. Proves the
simulator *relays* local chat between distinct agents (vs `chat-self-echo`'s
self-echo). Green on OpenSim; relay RTT ≈ 1 ms on loopback.
