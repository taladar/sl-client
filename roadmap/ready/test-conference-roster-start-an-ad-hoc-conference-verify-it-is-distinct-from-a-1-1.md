---
id: test-conference-roster
title: start an ad-hoc conference; verify it is distinct from a 1:1 (multi-pa
topic: test
status: ready
origin: TEST_ROADMAP.md — Phase 3 — Instant messaging & chat sessions `[both]`
---

Context: [context/test.md](../context/test.md).

`conference-roster` — start an ad-hoc conference; verify it is distinct
from a 1:1 (multi-party roster, `SessionAdd`/`SessionLeave`). **`3av`,
`[aditi]` only → fully deferred to Phase Z.** Investigated 2026-06-30:
OpenSim has **no ad-hoc (non-group) conference support whatsoever**, so the
whole behaviour this case verifies is unobservable there. An ad-hoc
conference is `IM_SESSION_CONFERENCE_START`
(`ImDialog::SessionConferenceStart` = 16) with conference
`SessionSend`/`SessionAdd`/`SessionLeave` carrying `from_group = false`.
`InstantMessageModule.OnInstantMessage` relays only
`MessageFromAgent`/`StartTyping`/`StopTyping`/`BusyAutoResponse`/
`MessageFromObject` and drops everything else (`default: return;`);
`GroupsMessagingModule` acts only on `fromGroup == true`; no module emits a
`ChatterBoxInvitation` / roster `SessionAdd`/`SessionLeave` for a non-group
session, and there is no conference module to enable. So on OpenSim the
conference start reaches no invitee and yields no server roster — only the
client-side `Conference`-kind registry entry, which is a session-model unit
test, not a live-grid check. The genuine multi-party-roster behaviour is
Second Life only and needs both a 2nd and a 3rd Aditi avatar — see Phase Z.
