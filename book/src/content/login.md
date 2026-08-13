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

## Failure, MFA, gates, and redirects

A login can fail (bad credentials, region down, account on hold). The response
then carries a machine-readable `reason` and a human-readable `message` —
plus, when the request asked for `extended_errors`, a `message_id`
localization key and its `message_args` substitutions (a suspension-end
`TIME`, a required-update `VERSION`). Failures surface as
`DisconnectReason::LoginFailed`.

Several rejections are really *gates* the client can clear by re-sending the
same login with one flag flipped, and the reference viewer does exactly that:

- **`tos`** — the terms of service changed; the message carries the text, and
  accepting re-sends the login with `agree_to_tos` set.
- **`critical`** — a critical grid message must be acknowledged; re-send with
  `read_critical` set.
- **`update`** / **`optional`** — the grid wants a newer viewer
  (`message_args["VERSION"]`).
- **`presence`** — the avatar appears to be logged in already; usually a stale
  ghost the next attempt clears (but the same reason code also covers
  non-retryable cases, so the message text disambiguates).

Second Life can also answer with an **MFA challenge** instead of success or
outright failure: the client must collect a one-time token from the user and
re-submit the login with it. This is a distinct third outcome, not a failure.

Finally, a grid can answer with a **redirect**: `login = "indeterminate"`
plus a `next_url` and `next_method`. The client re-POSTs the identical
request struct to `next_url` (as an XML-RPC call named `next_method`) and
keeps going until a terminal response arrives — our drivers bound the loop at
a handful of hops.

TOTP tokens are valid only within a wall-clock-aligned 30-second window, so a
token generated near the end of a window can expire mid-flight. The
[REPL test client](../tools/sl-repl.md) handles this by reading credentials from
a TOML file (with an optional `mfa_command` per avatar) and, when too little of
the current window remains, waiting out its tail before generating the token so
the re-submitted login survives the round-trip.

## The server side

The same codec works in the grid direction, so a login *server* (a fake grid,
a test harness) can serve an unmodified viewer:

- `parse_login_request` reads the struct a viewer POSTs (every
  identification field the reference viewer sends, the acknowledgement
  booleans, the `options` list) without ever trusting it — a malformed
  `start` is preserved verbatim rather than dropped.
- `LoginServer::respond` maps the parsed request plus the account facts to
  the response, enforcing the checks in the order the real grids exhibit:
  **redirect → password → ToS → critical message → MFA → presence →
  success**. The gates are data (`LoginGates`), so a fake grid scripts them
  per scenario.
- `build_login_response` serializes any outcome — including every optional
  success section — such that our own parser (and the reference viewer's)
  reads back exactly what was meant. A server that honours the request's
  `options` list applies `LoginSuccess::filter_options` first; OpenSim
  ignores the list and sends everything.

## Why XML-RPC — and the LLSD variant

Login predates CAPS, which is why its primary codec is XML-RPC rather than
[LLSD](../comms/llsd.md) over HTTP like everything else on the HTTP side. It
is the only XML-RPC call in the protocol; once you are logged in, the HTTP
side is all LLSD-over-CAPS.

Grids do, however, accept an **LLSD login** at the same URL, selected purely
by the POST's `Content-Type`: `text/xml` means XML-RPC,
`application/llsd+xml` means LLSD (OpenSim wires it as its default LLSD
handler; Second Life accepts it too). The reference viewer only ever sends
XML-RPC, so the LLSD variant matters mainly to a login *server* that wants to
accept any conformant client. Both variants carry the identical field set;
only the representation differs (native uuid/integer/boolean values, and the
config-like sections as a one-element array wrapping a map).

---

> **In this codebase**
>
> - The XML-RPC request builder and response parser are in
>   `sl-wire/src/login.rs` (a pure codec — no I/O). The response is modelled
>   as a success / MFA-challenge / redirect / failure union.
> - The server direction lives alongside them: `parse_login_request`,
>   `LoginGates`, `LoginServer::respond`, and `build_login_response`.
> - The LLSD variant is `sl-wire/src/login_llsd.rs`
>   (`build_login_request_llsd`, `parse_login_request_llsd`,
>   `build_login_response_llsd`, `parse_login_response_llsd`).
> - The login parameter and result types (`LoginParams`, `LoginAccount`,
>   `LoginHttpRequest`) live in `sl-proto/src/types/session.rs`. The `Session`
>   consumes the parsed response and establishes the circuit; the login
>   follow-up surfaces as `Event::Account(..)`, `Event::InventorySkeleton(..)`,
>   `Event::LibraryInventory(..)`, and `Event::FriendList(..)`. A redirect
>   keeps the session fresh and re-arms `login_http_request()` at the new
>   endpoint.
> - The actual HTTPS POST is done by the driver — see the login flow in
>   `sl-client-tokio/src/lib.rs` (redirects bounded by
>   `MAX_LOGIN_REDIRECTS`) and the example
>   `sl-client-tokio/examples/tokio_login_hold_logout.rs`.
> - `DisconnectReason::LoginFailed { reason, message }`
>   (`sl-proto/src/types/session.rs`) reports a rejected login;
>   `LoginFailure::kind()` classifies the retry-guiding reasons
>   (`Tos`, `CriticalMessage`, `UpdateRequired`, `AlreadyLoggedIn`).
