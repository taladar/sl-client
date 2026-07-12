---
id: test-chat-invite-accept-decline
title: AcceptChatInvite / DeclineChatInvite and the CAPS ChatSessionRequest p
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`chat-invite-accept-decline` — `AcceptChatInvite` /
`DeclineChatInvite` and the CAPS `ChatSessionRequest` path on SL. `2av`
(OpenSim now; Aditi deferred → Phase Z). A pending invite is a chat-session
registry entry whose lifecycle is `Invited`; accepting promotes it to `Joined`
and declining removes it. To provoke a *real* invitation the primary creates a
throwaway open-enrollment group, the secondary joins it as a member, and the
primary opens the group session and sends one message — which the secondary,
not yet a session participant, receives as a CAPS `ChatterBoxInvitation`
(`Event::ConferenceInvited` with `from_group`, the same not-yet-joined path
`group-session-message` documents). The case does this twice (one group to
accept, one to decline, since a second message in the same group arrives as a
plain session message), then drives accept / decline on the secondary and
asserts the registry via `QueryChatSessions`: `Invited` (inviter = primary,
text channel) before answering, `Joined` after `AcceptChatInvite`, and gone
after `DeclineChatInvite`. OpenSim exposes no `ChatSessionRequest` capability,
so there the accept is the optimistic local join and the decline a UDP
`SessionLeave` the module ignores — both observable only as the client-side
registry transition; asserting the cap POST and its reply roster is the Aditi
Phase Z variant. Surfaced and fixed an over-promotion bug: OpenSim sends a
`ChatterBoxSessionAgentListUpdates` (an informational voice roster push)
alongside the invitation, and the handler used the *promoting*
`chat_session_mut`, so the `Invited` window collapsed to `Joined` before any
accept; it now folds the roster via the non-promoting `chat_session_get_mut`,
keeping the lifecycle until an explicit accept or real session traffic. Green
on OpenSim; invite RTT ≈ 70–100 ms loopback. `[opensim]` only.
