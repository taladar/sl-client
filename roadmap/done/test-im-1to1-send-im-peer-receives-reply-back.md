---
id: test-im-1to1
title: send IM, peer receives; reply back
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`im-1to1` — send IM, peer receives; reply back. `2av`
(OpenSim now; Aditi deferred → Phase Z). A direct IM is an
`ImprovedInstantMessage` with the `IM_NOTHING_SPECIAL` dialog
(`ImDialog::Message`), routed by the grid's IM service rather than broadcast
to the region like local chat. The primary `Command::InstantMessage`s the
secondary (`avatar2`), which — a separate session — observes the
matching `Event::InstantMessageReceived` attributed to the primary, then
replies with its own IM, and the primary observes the matching reply. Each
direction tags its text with the sender's agent id so the predicate ignores
unrelated background IM; the case asserts `ImDialog::Message` plus
`from_agent_id`/`to_agent_id` in both directions, proving the service
delivers a *targeted* message (vs `chat-hear-other`'s proximity broadcast)
and that the reply travels back the same way. Green on OpenSim; deliver RTT
≈ 14 ms, reply RTT ≈ 0.4 ms on loopback.
