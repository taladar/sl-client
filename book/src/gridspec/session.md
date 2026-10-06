# Session

What each grid does with a session's circuits while it lasts, and with the
session when it ends. The session's states are described in
[Sessions](../comms/sessions.md), the circuit and its reliability in
[Circuits](../comms/circuits.md) and
[LLUDP transport](../comms/lludp-transport.md).

## Circuits

Both ends of a circuit ping each other, acknowledge each other's reliable
packets and send again what was not acknowledged. What a simulator does when
the client stops doing its half can only be seen by a client that stops and
keeps listening, so the session can be told to: `Command::ProbeCircuits`
(`probe_circuits` in the REPL) reports every inbound datagram as a
`Diagnostic::Datagram` and, at its stronger levels, withholds every
acknowledgement or transmits nothing at all.

Measured on 2026-10-06 on Second Life's beta grid (aditi) and the local
OpenSim standalone (a 2×2 block of regions), by:

- `keepalive-ping`, which watches both ends' pings for 32 seconds — on aditi
  from the sandbox region (no neighbours) and from a region with five;
- `circuit-unacked-resend`, which settles for twelve seconds, withholds every
  acknowledgement for 45, asks for a `RegionInfo` and counts its arrivals
  (aditi twice, OpenSim once);
- `circuit-silence`, which settles, transmits nothing until the simulator has
  been quiet for 30 seconds, then speaks again, and finally logs in afresh
  (aditi from both regions, OpenSim three times);
- `throttle-set`, which asks for up to 300 of the region's prims again under
  the 1000 kbps and the 50 kbps throttle preset and measures the object
  updates that come back.

All four hold aditi, OpenSim and both fake flavours to these answers, except
where a row says the fake grid does not model it.

### Pings

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| the client's ping round trip | 0.17–0.18 s | 0.2 ms | 0.15 ms; `FakeGridBuilder::link_latency` adds a delay each way |
| the simulator's `StartPingCheck` on the root circuit | every 5.1 s (gaps of 4.8–5.25 s) | every 5.28 s, to the hundredth | every 5.00 s, on both |
| on a child circuit | every 5.0 s, on each of five | every 5.28 s, on each of three | every 5.00 s |
| pings to a client that answers none | go on at the same cadence until the circuit is given up | the same | the same |

OpenSim's five seconds are ten half-second ticks of its outgoing-packet loop,
each of which runs a little long (`LLUDPServer.OutgoingPacketHandler`).

### A reliable packet nobody acknowledges

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| transmissions of the unacknowledged `RegionInfo` | **four**: the first and three more, then given up | **140 in 45 s**, and still going when acknowledgements resumed | `FakeSl` four; `FakeOpensim` without limit |
| the gap between them | 1.00–1.32 s | 0.30–0.42 s (median 0.32 s) | `FakeSl` 1.00 s; `FakeOpensim` 0.25 s on loopback |
| how a retransmission is marked | the same sequence number, the `RESENT` flag | the same | the same |
| the circuit afterwards | unaffected: it answered the next ping | unaffected | unaffected |
| which reliable packets are retransmitted at all | **few**: of 5,844 sent reliably in the 45 s, six — `RegionInfo` and `SimulatorViewerTimeMessage` (and `KillObject`, in the silent runs) | every one: all 13 | every one |
| unacknowledged object traffic | never retransmitted under its sequence number; the state is sent again as new packets — `AvatarAnimation` 1,435 times in the 45 s, against 74 in 32 s with acknowledgements flowing | retransmitted like anything else (`ImprovedTerseObjectUpdate`, `LayerData`) | not modelled: the fake grid resends what it sent |

Second Life's numbers are the reference library's
(`LL_DEFAULT_RELIABLE_RETRIES` 3, `LL_MINIMUM_RELIABLE_TIMEOUT_SECONDS` 1),
which is also what the session uses for its own packets. Its resends were a
second apart twelve seconds after an arrival, though, which the reference's
one-second initial ping average would not yet have decayed to: the
simulator's estimate starts lower.

OpenSim's retransmission timeout is five times the last ping round trip,
held between 250 ms and 3 s (`LLUDPClient.UpdateRoundTrip`), checked on a
100 ms tick, and nothing counts the resends (`LLUDPServer.ResendUnacked`): a
packet is resent until it is acknowledged or the client is timed out. A
client that takes its time acknowledging is sent the same packet three times
a second meanwhile, and has to discard the copies — which the session does,
by sequence number.

### A client that goes silent

The silence is of the circuits only: the event queue is HTTP and went on being
polled throughout.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| pings during the silence | every 5.1 s, 19 of them, the last at 93–94 s | every 5.28 s, 11 or 12, the last at 54–58 s | the same cadence to the end, on both |
| other traffic during the silence | goes on: about 23 datagrams every ten seconds once the scene has arrived (`CoarseLocationUpdate`, `LayerData`, the time message) | goes on, and grows: every unacknowledged packet is resent three times a second (160–170 transmissions of one packet) | pings only |
| the simulator's last datagram on the root circuit | at 97.5–98.9 s | at 59.1 s | `FakeSl` 98.0 s; `FakeOpensim` 59.0 s |
| what it says as it gives up | **nothing**: no `KickUser`, no `DisableSimulator`, no `CloseCircuit` | **`KickUser`**, "Simulator logged you out due to connection timeout." | each flavour's own |
| the child circuits | pinged until 98.3 s, last datagram at 98.5 s, nothing said | last datagram within 50 ms of the kick, nothing said on them | closed with the root's, nothing said |
| the client speaking again afterwards | no answer in 15 s | — (the kick ended the session) | no answer from `FakeSl` |
| a login five seconds later | admitted (after the two-minute login cooldown the harness keeps on aditi) | admitted | admitted on both |
| a login in the same instant | not measured | **refused** as `presence`, once: the close had not finished | not modelled |

So Second Life times a circuit out 100 seconds after the last datagram it
received (the silence began about a second after the client's last one), and
OpenSim after 60 (`AckTimeout`, `LLUDPServer.HandleUnacked`), measured from the
last packet received and not, despite the name, from anything unacknowledged.
OpenSim's kick is addressed to a client it has just decided is not there; a
client whose uplink alone has failed does receive it. Second Life's client
finds out from the silence: the session declares a circuit dead 45 seconds
after the last datagram it received, and reports `Disconnected(Timeout)`.

The refusal of an immediate login is the race described under
[Login](login.md#a-second-login-of-an-avatar-that-is-in-world): OpenSim's
close runs on another thread, and the login service still held the presence.

### Throttles

Both grids acknowledge an `AgentThrottle` and answer it with nothing.

| behaviour | Second Life | OpenSim | fake grid |
| --- | --- | --- | --- |
| object updates under the 1000 kbps preset (task rate 310 kbps) | 539 KB in 15 s — 288 kbps, with the sandbox's scene still arriving | 109 prims, 12.7 KB, half of it within 0.20 s | at once |
| under the 50 kbps preset (task rate 10 kbps) | 256 KB in 30 s — **68 kbps** | 15.2 KB in four times as many datagrams, half of it after 6.6 s — **9.2 kbps** | at once: throttles are not honoured (`server-world-update-scheduling`) |

OpenSim holds a category to the rate it was given (its token buckets are in
`LLUDPClient.cs` and `TokenBucket.cs`) and cuts its datagrams smaller to do
it. Second Life sends less when asked for less, but did not go down to what
the preset asked for.

### The short zero-coded tail

A few times per login, Second Life's simulators zero-code an `ObjectUpdate`
with its final run of zeros counted one byte short (usually an encoded body
ending `00 42` where the block's trailing fields need 67 zeros), and the
reference reader zero-fills what is missing. The session reads such a body
the same way ([LLUDP transport](../comms/lludp-transport.md#zero-coding)).
`FakeSl` sends **every** `ObjectUpdate` that way
(`CircuitPolicy::short_zero_tails`) — more often than the live grid, so that
each test which sees an object on that flavour is sure to read one;
`FakeOpensim` sends them as encoded.

### What the viewer does with it

- The session discards a retransmission it has already processed and
  acknowledges it again, on both grids' cadences; `circuit-unacked-resend`
  runs against both fake flavours on every `cargo test`.
- A circuit timed out by OpenSim reaches the viewer as a kick, and one timed
  out by Second Life as the session's own `Disconnected(Timeout)`; the viewer
  exits on either (`viewer-disconnect-screen` keeps the window open instead).
- The e2e tests `a_distant_second_life_flavoured_grid_is_logged_into_and_left`
  and `a_distant_opensim_flavoured_grid_is_logged_into_and_left`
  (`e2e_login.rs`) log in, load the Library and quit with every datagram held
  85 ms each way — the round trip measured to aditi.
- `Command::RequestObjects` for more than 255 objects used to fail to encode
  — a message's block count is one byte — and the driver ended the session
  over it. It is split across messages now. The same limit applies to every
  other request that lists objects (`protocol-variable-block-lists-over-255`).

### Not measured

| behaviour | why |
| --- | --- |
| a silence that includes the event queue | the probe holds the circuits; the driver's HTTP goes on |
| a child circuit that goes silent while the root answers | the probe holds every circuit of the session |
| whether Second Life lets a dropped avatar log in at once | the harness waits out aditi's two-minute login cooldown first |
| OpenSim's longer timeout for a paused agent | from source only: 300 s after an `AgentPause` (`PausedAckTimeout`) |
| how far down Second Life's throttle goes, and per category | one burst under two presets; the scene was still arriving under both |
| which messages each grid zero-codes besides `ObjectUpdate` | the probe reports a datagram's flags, but the cases do not tally them |
| loss, reordering and duplication on the way | the probes drop the client's own traffic, never the simulator's |

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
