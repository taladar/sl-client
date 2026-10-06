# Session

What each grid does with a session that is ending. The session's states are
described in [Sessions](../comms/sessions.md).

## Logout

A logout is one reliable `LogoutRequest` on the root circuit and — when the
grid answers — one `LogoutReply`. The client waits five seconds for it
(`sl_proto::LOGOUT_TIMEOUT`) and then ends the session itself; either way it
reports `Event::LoggedOut`, and only a `Diagnostic::ExpectedReplyMissing`
tells the two apart.

Measured on 2026-10-06 on Second Life's beta grid (aditi) and the local
OpenSim standalone (a 2×2 block of regions), by:

- `logout-clean`, which logs out **settled** (twelve seconds in world, child
  circuits open), makes exactly one login request, and logs that session out
  **early** — the moment its region handshake completes;
- `logout-seated`, in which one avatar rezzes a cube, sits on it and logs
  out while a second avatar watches, then logs in again at `last` (OpenSim
  only: four runs, the same answer each time);
- 21 scripted `sl-repl` logouts on OpenSim after holds of 0, 1, 2, 3, 5, 8
  and 15 seconds, three of each, read off the wire trace
  (`sl_proto::wire`), and five more runs of `logout-clean` there; and one
  scripted logout on aditi from a region with five neighbours.

`logout-clean` holds aditi and both fake flavours to these answers as
`Measured` constants. The live OpenSim's reply is a race (below), so there
the case records it and asserts only what does not vary.

### The reply

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| `LogoutReply` to a settled logout | sent, every time, 0.17 s after the request (0.07 s from the region with neighbours) | **sent in 5 of 20** logouts made three seconds or more after arriving, within 3 ms; otherwise nothing | `FakeSl` sends it; `FakeOpensim` withholds it |
| `LogoutReply` to a logout made as the region handshake completes | sent, 0.18 s | **never**: 0 of 15 made within two seconds of arriving | as above |
| the `LogoutRequest`'s acknowledgement when no reply comes | — | none: the root circuit sends nothing at all after the request | `FakeOpensim` sends nothing either |
| `InventoryData` in the reply | settled: four item ids (on both accounts measured); early: one block holding the nil id | one block holding the nil id | one block holding the nil id, on both |
| what the client does without a reply | — | resends the request unanswered, and reports `LoggedOut` at five seconds | the same against `FakeOpensim` |

OpenSim's handler (`LLUDPServer.LogoutHandler`) queues the reply on the
`Task` throttle's outbox and then closes the agent in the same call.
`LLClientView.CloseWithoutChecks` calls `LLUDPServer.Flush` — a stub
containing `// FIXME: Implement?` — and then `LLUDPClient.Shutdown`, which
clears every outbox and the pending acknowledgements. The reply reaches the
wire only when the outgoing-packet thread drains the outbox between those two
steps. Just after arriving it never does, because the outbox is still full
of the arrival burst; later it is a matter of timing. The request's
acknowledgement rides out with the reply or not at all, which is why a
client that resends reliable packets sometimes logs the `LogoutRequest` as
having exhausted its retransmissions just before its logout timeout fires.

The close itself always happens, reply or no reply: OpenSim's log shows the
agent removed and the presence logged out within the same second.

The reference viewer reads the reply's `InventoryData` as the items whose
folders changed on the way out (`process_logout_reply`), treating a single
nil id as an empty list. The session does not read the block
(`inventory-logout-reply-items`).

### Child circuits

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| child circuits open at the logout | none in the test avatars' sandbox region; five in the region the third account stands in | three (every other region of the block) | one: the region east of the start region |
| what they are sent | nothing before the `LogoutReply` | `DisableSimulator` on each, 5–70 ms after the request | `DisableSimulator` on each, on both flavours |
| what the root circuit is sent besides the reply | nothing | nothing — no `DisableSimulator`, no `KickUser` | nothing |

What Second Life sends down a child circuit *after* its reply was not
measured: the client ends the session on the reply, as the reference viewer
does, so nothing sent later can reach either. The `DisableSimulator`s were
only seen on OpenSim because the reply did not come and the client was still
listening.

### The avatar and the next login

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the next login, one request | admitted (after the two-minute login cooldown the harness keeps on aditi) | admitted at once, whether or not the logout was answered | admitted on both |
| a seated avatar, as an observer sees it | not measured (below) | the seat is re-sent once, then the avatar's `KillObject`, 30 ms after the request | not modelled: residents of the fake grid are not shown to each other (`server-fake-grid-agent-avatars-shared`) |
| the seat after the kill | not measured | not re-sent | — |
| the next login at `last` after a seated logout | not measured | standing, 1 m from the seat | — |

So a logout that OpenSim never answered left nothing behind: no presence to
refuse the next login, no avatar in anybody's view. The `presence` refusal
that OpenSim gives a session which ended *without* a `LogoutRequest` is in
[Login](login.md#a-second-login-of-an-avatar-that-is-in-world).

### What the viewer does with it

- A quit sends the `LogoutRequest` and exits when the session reports
  `LoggedOut`. Its own deadline for forcing the exit is two seconds *longer*
  than the session's logout timeout. It used to be three seconds — shorter —
  which ended every unanswered quit by force, before the session had saved
  the inventory cache it writes on the way down.
- The e2e tests `a_second_life_flavoured_quit_exits_cleanly` and
  `an_opensim_flavoured_quit_exits_cleanly` (`e2e_login.rs`) quit with the
  menu's chord on each flavour and hold the viewer to a clean exit and the
  grid to holding no session.
- During the five seconds an unanswered logout takes, nothing on screen says
  the quit was heard; the reference shows "Logging out..."
  (`viewer-quit-progress`).

### Not measured

| behaviour | why |
| --- | --- |
| what Second Life sends on child circuits after the `LogoutReply` | the client has ended the session by then, as the reference viewer has |
| which items the reply's `InventoryData` names on Second Life | four ids on each of two accounts; not compared with what the avatars wear (`inventory-logout-reply-items`) |
| a seated logout on Second Life | aditi never answered the sit: three `logout-seated` runs got the cube rezzed and seen by both avatars, sent `AgentRequestSit` for it, and drew neither an `AvatarSitResponse` nor an alert within the session's sit timeout — with the cube 1.5 m from the avatar at its own height, and again at ground level. Why is `gridspec-sit-stand`'s to find out; the case's Second Life answers are the reference's expectation, not a measurement |
| a logout while a teleport or a region crossing is in flight | left to the teleport and crossing chapters |
