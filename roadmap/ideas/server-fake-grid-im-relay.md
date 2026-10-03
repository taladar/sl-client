---
id: server-fake-grid-im-relay
title: Fake grid — relay IMs and offers between its sessions
topic: server
status: ideas
origin: test-e2e-live-verify-sweep (2026-09-30)
refs: [test-e2e-sweep-two-avatars, server-message-routing]
blocked_by: [gridspec-teleport-lures, gridspec-instant-messages]
---

Context: [context/testing.md](../context/testing.md).

An `ImprovedInstantMessage` a viewer sends reaches its `SimSession` as a
`ServerEvent::InstantMessage` and goes no further: the fake grid does not
deliver it to the recipient's session. So two stage viewers cannot IM each
other, offer each other items, friendship or a teleport, or see each other
typing, and a test that needs the other side scripts it through
`FakeAgent::with_sim` instead (the do-not-disturb test does). The relay is
`SimSession::send_instant_message` on the recipient's root session, found by
agent id — the one-grid, one-process subset of
[[server-message-routing]], without offline storage or cross-host routing.
