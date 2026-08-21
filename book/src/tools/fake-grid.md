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
  stores (inventory, parcels, features, …) and greets arriving avatars;
- the niche non-CAPS HTTP surfaces a grid manager and the world map
  expect next to the login URI (see below): `get_grid_info`, the
  map-tile files, and the economy helper scripts.

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

## The non-CAPS HTTP surfaces

Besides login and CAPS, a real login host answers three more things a
viewer asks for, all served from the same loopback port (the sans-I/O
codecs live in `sl-wire`: `grid_info`, `map_tile`, `economy_helper`,
over a small generic `xmlrpc` module):

- **`GET /get_grid_info`** — the `<gridinfo>` document Firestorm's grid
  manager fetches before it even shows the login screen (it resolves the
  grid's name, nickname, platform, and helper URI from it). The same
  entries answer the XML-RPC method `get_grid_info` POSTed to `/`, as
  OpenSim does. `FakeGridBuilder::grid_identity` / `--grid-name` /
  `--grid-nick` set the name and nickname; the `economy` (helper URI)
  entry is the login URI itself.
- **`GET /map-<zoom>-<x>-<y>-objects.jpg`** — world-map tiles, in the
  file-name shape `sl-map-apis` and the viewer's world map request. The
  login response's `map-server-url` and the stock `SimulatorFeatures`
  `OpenSimExtras` both point at the login URI, so a viewer's world map
  loads tiles from the fake grid. Every configured region gets a stock
  zoom-1 tile (an embedded JPEG); `FakeGridBuilder::map_tile` registers
  others. Absent tiles are 404; tiles carry `Cache-Control`/`ETag` so the
  viewer's disk cache holds them.
- **`POST /currency.php`** and **`POST /landtool.php`** — the XML-RPC
  economy helpers behind the buy-L$ and buy-land floaters
  (`getCurrencyQuote`/`buyCurrency`, `preflightBuyLandPrep`/`buyLandPrep`).
  `EconomyConfig` sets the currency symbol, the price (US cents per
  1000 L$), whether the "site" is up, and whether land purchases demand
  a membership / land-use upgrade; a quote hands out a `confirm` token
  the commit must echo. Nothing moves a balance — an accepted purchase
  is published on `FakeGrid::economy_events` for tests to assert.

## The legacy UDP asset fixtures

`SimSession` implements the server half of the legacy UDP asset paths but
holds no content; `Scenario::udp_assets` (`UdpAssetFixtures`) is where a
scenario scripts it, and the driver answers the matching `ServerEvent`s
from a per-session copy, under the same lock and flush rule as everything
else:

- **Named `Xfer` files** (`xfer_files`) — registered on every fresh
  session and re-armed after each serve, since a `SimSession`
  registration is consumed by the `RequestXfer` that names it. An unknown
  name gets the machine's own `AbortXfer`.
- **Task inventories** (`task_inventories`, by region-local object id) —
  answered with `serve_task_inventory` on `RequestTaskInventory`; an
  unknown id is ignored, as a real simulator ignores a bogus one.
- **`TransferRequest` sources** — task-item asset bodies by `(task, item)`
  (`task_item_assets`) and the estate covenant notecard
  (`estate_covenant`); a miss is refused with `UnknownSource`, which the
  client surfaces as `TransferFailed` instead of hanging.
- **The terrain RAW heightmap** (`terrain_raw`) — offered with
  `send_initiate_download` on an estate "download filename" request, and
  *replaced* by a completed upload (`request_xfer_upload` → `XferReceived`),
  so a download after an upload round-trips the uploaded bytes. A "bake"
  request is acknowledged as an event only: the fake grid keeps no revert
  baseline. `flat_terrain_raw(height_m)` builds a flat 256 × 256 RAW32
  file for fixtures.

The stock scenario ships `motd.txt`, one scripted object
(`STOCK_SCRIPTED_OBJECT_LOCAL_ID`) whose task inventory holds a script with
a body, a covenant, and a flat 25 m heightmap. Behaviour the fixtures do
not cover goes in `Scenario::on_event`, a hook that sees every drained
`ServerEvent` with the live `SimSession` (after the stock behaviour ran).
`client_end_to_end.rs` drives each of these flows through the real
`sl-client-tokio` commands.

The stock `Scenario` is intentionally small (an inventory skeleton, a
library, one parcel, a chat greeting). A real viewer will ask for much
more — terrain, appearance, nearby objects — and renders a login into an
empty world; growing the default scenario against what a viewer actually
requests is expected iteration, not a bug. Firestorm's seed-request
retries (up to 30×) are harmless: the grant is minted once, so every
retry gets a byte-identical reply.
