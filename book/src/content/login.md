# Login

Login is the one part of the protocol that happens *before* there is a
[session](../comms/sessions.md), a [circuit](../comms/circuits.md), or any
[CAPS](../comms/caps.md). It is an **XML-RPC** call over HTTPS to the grid's
login service (historically `login.cgi`), and its response bootstraps everything
else.

## The request

The client POSTs an XML-RPC `login_to_simulator` call carrying, among other
things:

- **first name / last name** (or, on Second Life, the account's username),
- the **password**, sent as an MD5 hash with a `$1$` prefix rather than in the
  clear,
- a **start location** — `"last"`, `"home"`, or a specific
  `uri:Region&x&y&z`,
- viewer **channel / name / version / platform / MAC / id0** fields, which grids
  use for statistics and gating.

## The response

A successful response is a large XML-RPC struct. The important parts:

- **Identity & session** — the `agent_id`, a freshly minted `session_id` and
  `secure_session_id`, and the **`circuit_code`** used to bring up the first
  [circuit](../comms/circuits.md).
- **Where you start** — the simulator's IP/port and the start/home positions, so
  the client knows where to send `UseCircuitCode`.
- **The seed capability** — the single CAPS URL from which all other
  [capabilities](../comms/caps.md#the-seed-capability) are fetched.
- **The inventory skeleton** — the *shape* of the avatar's
  [inventory](inventory.md) (every folder's id, name, parent, type, and version)
  but not its contents, plus the separate library skeleton.
- **The buddy list** — the avatar's [friends](friends.md) and the rights each
  side has granted.
- **Account limits** — maturity/access level, and assorted per-account flags.

After parsing this, the client transitions from `New` to `AwaitingHandshake`
(see the [session lifecycle](../comms/sessions.md#the-lifecycle)) and sends
`UseCircuitCode` + `CompleteAgentMovement` to the simulator named in the
response.

## Failure and MFA

A login can fail (bad credentials, region down, account on hold). The response
then carries a machine-readable `reason` and a human-readable `message`,
surfaced as `DisconnectReason::LoginFailed`.

Second Life can also answer with an **MFA challenge** instead of success or
outright failure: the client must collect a one-time token from the user and
re-submit the login with it. This is a distinct third outcome, not a failure.

TOTP tokens are valid only within a wall-clock-aligned 30-second window, so a
token generated near the end of a window can expire mid-flight. The
[REPL test client](../tools/sl-repl.md) handles this by reading credentials from
a TOML file (with an optional `mfa_command` per avatar) and, when too little of
the current window remains, waiting out its tail before generating the token so
the re-submitted login survives the round-trip.

## Why XML-RPC, and only here

Login predates CAPS, which is why it uses XML-RPC rather than
[LLSD](../comms/llsd.md) over HTTP like everything else on the HTTP side. It is
the only XML-RPC call in the protocol; once you are logged in, the HTTP side is
all LLSD-over-CAPS.

---

> **In this codebase**
>
> - The XML-RPC request builder and response parser are in
>   `sl-wire/src/login.rs` (a pure codec — no I/O). The response is modelled as
>   a success / MFA-challenge / failure union.
> - The login parameter and result types (`LoginParams`, `LoginAccount`,
>   `LoginHttpRequest`) live in `sl-proto/src/types/session.rs`. The `Session`
>   consumes the parsed response and establishes the circuit; the login
>   follow-up surfaces as `Event::Account(..)`, `Event::InventorySkeleton(..)`,
>   `Event::LibraryInventory(..)`, and `Event::FriendList(..)`.
> - The actual HTTPS POST is done by the driver — see the login flow in
>   `sl-client-tokio/src/lib.rs` and the example
>   `sl-client-tokio/examples/tokio_login_hold_logout.rs`.
> - `DisconnectReason::LoginFailed { reason, message }`
>   (`sl-proto/src/types/session.rs`) reports a rejected login.
