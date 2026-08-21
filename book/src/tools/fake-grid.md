# The fake grid

`sl-fake-grid` is an in-process loopback grid built from the workspace's
sans-I/O server machinery. The protocol logic all lives elsewhere —
`sl-wire`'s login server, `sl-proto`'s `SimSession` (the simulator-side
protocol machine, see [Sessions](../comms/sessions.md)) and `SimCaps`
(the capability dispatch, see [CAPS](../comms/caps.md)) — this crate adds
only the I/O those cores deliberately leave to a runtime:

- an HTTP endpoint (hyper) serving the login POST at `/` — both codecs at
  one URL, XML-RPC for `text/xml` and LLSD for `application/llsd+xml` —
  and every session's CAPS surface under `/sim/<n>/cap/<token>`;
- the `EventQueueGet` long-poll hold: an empty poll is kept open
  (default 30 s, configurable) and woken by the next enqueued event, or
  answered with the 502 the viewer reads as "nothing yet, re-poll";
- one loopback UDP socket per logged-in session, pumped into
  `SimSession::handle_datagram` with the machine's own `poll_timeout`
  deadlines driven by a timer task (acks, resends, pings, inactivity);
- scriptable content fixtures — a `Scenario` seeds each fresh session's
  stores (inventory, parcels, features, …) and greets arriving avatars.

There is deliberately **no world authority**: no physics, no persistence,
no inter-client broadcast beyond what a test scripts. The fake grid is
"real I/O glue, scripted content, no authority" — the half-way point
between the in-memory loopback tests (`sl-proto/tests/sim_session.rs`)
and a real grid.

## The driver invariant

One logged-in avatar is one `SimSession` + `SimCaps` pair behind one
async mutex. Every path that mutates the machine — the UDP pump, the
timer, a CAPS dispatch, a test's `with_sim` call — ends with the same
flush sequence: drain `poll_event()` into the session's `ServerEvent`
broadcast (running the automatic `RegionHandshake` on `AgentArrived`),
collect `poll_transmit()` datagrams, republish the `poll_timeout()`
deadline, and wake a held event-queue poll if events are queued. Socket
I/O happens only after the lock is released, so nothing ever awaits
while holding the state.

## As a library

```rust,ignore
let grid = FakeGridBuilder::new()
    .account(AccountConfig::new("Test", "User", "password"))
    .region(RegionConfig::default())
    .start()
    .await?;
let mut logins = grid.logins();
// Point any client at grid.login_uri(), then:
let agent = grid.agent(&logins.recv().await?).await.unwrap();
agent.with_sim(|sim| sim.send_chat_from_simulator(/* … */)).await;
let mut events = agent.events();   // the grid-side ServerEvent stream
```

`with_sim` is the only sanctioned way to call `send_*` / `set_*` /
`enqueue_*` on a live session — it runs the closure under the lock and
then flushes, so the datagrams actually leave and a held event-queue
poll actually wakes. Everything under `sl-fake-grid/tests/` works this
way: `http_glue.rs` drives the endpoints with bare `reqwest` plus the
client-direction `sl-wire` codecs; `client_end_to_end.rs` runs the real
`sl-client-tokio` stack — login POST, UDP circuit, seed fetch,
event-queue long-poll — against it.

## As a standalone grid

```sh
cargo run -p sl-fake-grid -- --http-port 9100 \
  --account 'Test:User:password'
```

logs `fake grid ready: login URI http://127.0.0.1:9100/`. Point this
workspace's clients at it (`SL_LOGIN_URI=http://127.0.0.1:9100/`), or
add it to Firestorm's grid manager as a grid with that login URI. With
no `--account` it creates `Test User` / `password`.

The stock `Scenario` is intentionally small (an inventory skeleton, a
library, one parcel, a chat greeting). A real viewer will ask for much
more — terrain, appearance, nearby objects — and renders a login into an
empty world; growing the default scenario against what a viewer actually
requests is expected iteration, not a bug. Firestorm's seed-request
retries (up to 30×) are harmless: the grant is minted once, so every
retry gets a byte-identical reply.
