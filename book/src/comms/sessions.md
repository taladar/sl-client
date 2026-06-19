# Sessions

A **session** is one logged-in presence of one avatar on a grid. It begins when
the [login](../content/login.md) service accepts your credentials and ends when
you log out (or get disconnected). Everything the client does — sending chat,
moving, fetching inventory — happens in the context of a session.

## The identifiers

A session is pinned together by a handful of UUIDs and one integer, all handed
out by the login service and then quoted back in nearly every message:

| Identifier | Meaning |
|------------|---------|
| **Agent ID** | The avatar's permanent UUID. Stable across sessions; this *is* the avatar's identity on the grid. |
| **Session ID** | A fresh UUID identifying *this* login. Together with the agent id it authenticates messages; it changes every login. |
| **Circuit code** | A 32-bit integer authorizing the UDP [circuit](circuits.md) to the region. |
| **Secure Session ID** | A secondary secret used by some operations. |

Most UDP messages that act on the world carry an `AgentData` block with the
agent id and session id, and the region rejects anything whose pair does not
match a session it knows about. This is the protocol's basic authentication: you
prove who you are by quoting the session id you were issued at login, over the
circuit whose code you were issued.

## The lifecycle

A session is a small state machine. From the client's point of view it moves
through these states:

```text
  New ──login accepted──▶ AwaitingHandshake ──RegionHandshake──▶ Active
                                                                  │  ▲
                                            TeleportLocationRequest│  │TeleportFinish
                                                                  ▼  │
                                                              Teleporting
   Active ──LogoutRequest──▶ LoggingOut ──LogoutReply──▶ Closed
```

- **New** — constructed; the login request is ready but not yet answered.
- **AwaitingHandshake** — login succeeded and the bootstrap packets
  (`UseCircuitCode`, `CompleteAgentMovement`) have been sent to the region; the
  client is waiting for the region to introduce itself with a `RegionHandshake`.
- **Active** — the handshake is done and keep-alive traffic (`AgentUpdate`,
  acks) is flowing. This is the normal steady state.
- **Teleporting** — a [teleport](../content/teleport.md) is in progress; the
  client is waiting for the destination to confirm.
- **LoggingOut** — a `LogoutRequest` was sent; the client is waiting for the
  `LogoutReply` so it can shut down cleanly.
- **Closed** — finished.

If things go wrong, the session reports *why* it ended rather than silently
dying: bad credentials, an inactivity timeout, a handshake whose reliable
packets ran out of retransmissions, or an unrecoverable wire error.

## Keep-alive

A session must produce traffic or the region will time it out. The client
periodically sends `AgentUpdate` (which also reports camera and control state)
and flushes acknowledgements for reliable packets it has received. Conversely,
if *no* traffic arrives from the region within an inactivity budget, the client
treats the session as timed out.

## Client and server views

The same lifecycle exists from the region's side. This workspace models both:
the client-side machine and a server-side mirror that accepts an incoming
circuit, completes the handshake, and tracks link health. The server-side view
exists mainly so the protocol can be exercised end-to-end in tests without a
real grid.

---

> **In this codebase**
>
> - The client state machine is `Session` in `sl-proto/src/session.rs`; the
>   internal `SessionState` enum has the variants `New`, `AwaitingHandshake`,
>   `Active`, `Teleporting`, `LoggingOut`, `Closed`.
> - The reason a session ended is `DisconnectReason`
>   (`sl-proto/src/types/session.rs`): `LoginFailed`, `Timeout`,
>   `HandshakeFailed`, `ProtocolError`.
> - The server-side mirror is `SimSession` in `sl-proto/src/sim_session.rs`.
> - Login/connection parameter types live in `sl-proto/src/types/session.rs`
>   (`LoginParams`, `LoginAccount`, …).
> - `Session` is a pure state machine: it is fed bytes and the current `Instant`
>   and emits [`Event`](../content/index.md)s; the actual socket work is done by
>   the driver crates (`sl-client-tokio`, `sl-client-bevy`).
