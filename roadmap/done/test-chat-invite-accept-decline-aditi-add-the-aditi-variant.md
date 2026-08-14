---
id: test-chat-invite-accept-decline-aditi
title: Chat invite accept/decline — [aditi] variant
topic: test
status: done
origin: TEST_ROADMAP.md — Phase Z — Deferred: multi-avatar Aditi work
---

Context: [context/test.md](../context/test.md).

The `[aditi]` variant of the `chat-invite-accept-decline` case
(`[[test-chat-invite-accept-decline]]`, already green `[opensim]`): **pass
(partial) on aditi live** (2026-08-12, Phase Z batch). The accept/decline
registry transitions are asserted on OpenSim; on Second Life the case
records an honest partial when it cannot provoke the invitation.

The invitation is provoked by sending a group-session message to a
not-yet-joined member — which hits the same Second Life behaviour as
[[test-group-session-message-aditi]]: a message into a freshly created
group is dropped until the group service has propagated. The invitation
send retries with a fresh marker (six attempts) and records a partial
("no group-session invitation was delivered within the retry budget")
when none is delivered. A pre-made propagated fixture group would let the
full `AcceptChatInvite` / `DeclineChatInvite` registry-transition
assertion run on SL — the remaining follow-up. Shares the fixes in
[[test-group-join-leave-aditi]].
