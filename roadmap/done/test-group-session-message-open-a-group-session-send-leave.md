---
id: test-group-session-message
title: open a group session, send, leave
topic: test
status: done
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`group-session-message` — open a group session, send, leave. `2av`
(needs Groups; OpenSim requires Groups V2). The first Phase 3 case to need a
setup beyond the avatars: it creates a throwaway open-enrollment group (the
primary becomes founder) and has the secondary `JoinGroup` it, so it depends
only on Groups V2 being enabled, not on any pre-existing group. Both avatars
then `StartGroupSession`, the primary `SendGroupMessage`s a marker tagged with
its own agent id, and the secondary — a fellow member — observes it. OpenSim's
`GroupsMessagingModule` delivers to a member who is already a session
participant as a UDP `IM_SESSION_SEND` (`Event::GroupSessionMessage`, the
canonical path the secondary's pre-join takes), but to a not-yet-joined member
as a CAPS `ChatterBoxInvitation` carrying the first message inline
(`Event::ConferenceInvited` with `from_group`); the predicate accepts either,
so a lost join/send race proves delivery rather than flaking, recording the
path taken. `LeaveGroupSession` has no observable OpenSim effect (the module
ignores the `SessionDrop` dialog over UDP), so the case confirms only that the
circuit survives the leave (a keep-alive ping still round-trips) — the
"acceptance = absence of failure" shape `throttle-set` uses. Green on OpenSim;
deliver RTT ≈ 9 ms loopback, via the `group-session-message` path. Required a
harness fix: each session's events are now forwarded off the run loop's
bounded channel into an unbounded one, so events that go unread (the
non-awaited avatar) never stalls its run loop — without it the primary's
just-queued `SendGroupMessage` sat untransmitted for ~30 s. `[opensim]` only;
Aditi deferred → Phase Z.
