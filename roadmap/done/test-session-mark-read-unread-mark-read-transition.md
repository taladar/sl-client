---
id: test-session-mark-read
title: unread → mark-read transition
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`session-mark-read` — unread → mark-read transition. `2av`
(OpenSim now; Aditi deferred → Phase Z). Each chat session carries an unread
counter, bumped on every inbound message that is not our own echo and cleared
either by our own outbound send or explicitly by `Command::MarkSessionRead`
(the viewer "marking a conversation read" without replying). The secondary
sends two 1:1 IMs to the primary — each tagged with the secondary's agent id
so the predicate ignores unrelated background IM — and the primary, a separate
session, observes both `Event::InstantMessageReceived` and so accumulates
`unread == 2` on its `Direct { peer: secondary }` session. The primary then
`MarkSessionRead`s that session and the transition is asserted against its
registry via `QueryChatSessions`: before marking, the session is a `Joined`
1:1 with `unread == 2` (two messages, proving the counter *counts* rather than
flips a has-unread flag); after marking it is still present and still `Joined`
(mark-read clears the badge, it does not close the conversation) but reads
`unread == 0`. `MarkSessionRead` is a purely local registry operation (no wire
send), identical on both grids; only the seeding IMs touch the wire (plain
LLUDP `ImprovedInstantMessage`). Green on OpenSim; deliver RTT ≈ 5 ms on
loopback. `[opensim]` only.
