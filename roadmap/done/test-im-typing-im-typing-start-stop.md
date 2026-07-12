---
id: test-im-typing
title: IM typing start/stop
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`im-typing` — IM typing start/stop. `2av`
(OpenSim now; Aditi deferred → Phase Z). IM typing is an
`ImprovedInstantMessage` with an `IM_TYPING_START`/`IM_TYPING_STOP` dialog and
the literal text `"typing"`, routed by the grid's IM service to one named
recipient and carrying the canonical 1:1 session id (`agent_id XOR
to_agent_id`) — the IM-session analogue of `typing-indicator`'s local-chat
broadcast. The primary `Command::ImTyping`s `typing: true` then
`typing: false` to the secondary (`Friend Tester`), which — a separate
session — observes the matching `Event::ImTyping`s attributed to the primary
on that session. The predicate matches the primary's `from_agent_id`, and the
case asserts the observed `session_id` equals the canonical id of the
secondary's `Direct` session with the primary, plus the `typing` flag in each
direction — proving the signal arrived on the targeted 1:1 session, not as a
stray broadcast. Where `im-1to1` proves the IM service relays a targeted
*message*, this proves it relays the typing *signal* over the same session.
Green on OpenSim; start RTT ≈ 0.7 ms, stop RTT ≈ 0.4 ms on loopback.
