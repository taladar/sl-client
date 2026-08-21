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
  stores (inventory, parcels, features, …), pushes the region's world
  (parcels, objects) at arriving avatars, and greets them;
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
broadcast (running the automatic `RegionHandshake` on `CircuitOpened` and
the arrival world burst on `AgentArrived`),
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

## The world fixtures

`Scenario::world` (`SceneFixtures`) holds the region's parcels
(`ParcelInfo`) and objects (`Object`) — the records the client decodes, so
a test asserts exactly what it seeded. A real simulator pushes a burst of
world state at an arriving viewer that nothing requested, and the driver
does the same on `AgentArrived`, right after `AgentMovementComplete`:

1. the agent's **own avatar object** (`pcode` `AVATAR`, the agent id as
   its full id, `FirstName`/`LastName`/`Title` name-values from the
   account) at the arrival point — the client's `current_parcel()` /
   `can_fly()` resolve the agent's parcel from this object's position;
2. the **parcel overlay** as `ParcelOverlay` chunks (one ownership byte
   per 4 m cell relative to the arriving agent, with the west/south
   parcel-edge bits, the way OpenSim's `SendParcelOverlay` builds it);
3. the **`ParcelProperties`** of the parcel under the arrival point
   (sequence id `0`, OpenSim's unsolicited-push convention);
4. one full **`ObjectUpdate`** carrying every fixture object.

Afterwards the same fixtures answer the client's `ParcelPropertiesRequest`
(by rectangle — answered from the rectangle's centre, echoing the sequence
id and snap flag) and `ParcelPropertiesRequestByID`, with a
`ParcelRequestResult::NoData` reply on a miss, and `RequestMultipleObjects`
with a fresh `ObjectUpdate` of the matching objects. The `SimSession`
helpers behind this (`send_parcel_properties`, `enqueue_parcel_properties`
for the CAPS event-queue form Second Life uses, `send_parcel_overlay`,
`send_object_update`, `send_object_update_compressed`, `send_kill_object`)
are also what a test's `with_sim` call uses to push world changes at a
live client. `region_wide_parcel(..)` and `box_prim(..)` build the
common fixtures.

The stock scenario's world is one region-wide public parcel
(`STOCK_PARCEL_NAME`, `STOCK_PARCEL_LOCAL_ID`, flying and rezzing
allowed) and the stock scripted object as a 1 m box at
`STOCK_SCRIPTED_OBJECT_POSITION` — so the task-inventory fixtures describe
an object a viewer can actually see and click.

**Ordering matters.** The `RegionHandshake` goes out on `CircuitOpened`
(`UseCircuitCode`), not on arrival: a viewer waits for the handshake
before it sends `CompleteAgentMovement`, and this workspace's client
discards a handshake that arrives after its `AgentMovementComplete`
already completed the arrival (it only listens while `AwaitingHandshake`).
The first version of the driver sent it on `AgentArrived` and the tokio
end-to-end test never noticed — it only waited for
`RegionHandshakeComplete`, which the movement-complete path also raises;
the Bevy smoke tier's `SlRegionIdentity` assertion is what caught it.

## The Bevy smoke tier

`sl-client-bevy/tests/fake_grid_login_smoke.rs` logs the real
`SlClientPlugin` — its socket-owning `sl-session-net` thread, the blocking
login, retransmission, the CAPS long-poll worker — into an in-process grid
from a `MinimalPlugins` app the test steps by hand (the grid's tasks run
on a tokio runtime the test owns; the frame loop never blocks on it, the
grid-side `ServerEvent` broadcast is drained with `try_recv`). It asserts
the whole pipeline in order: login → `CircuitEstablished` →
`RegionHandshakeComplete` → `SlIdentity`; the `maintain_world` state (one
`SlCurrentRegion` with the stock `SlRegionIdentity`, a complete
`SlParcelOverlay`, the stock parcel as `SlAgentParcel.current` and as the
region's `SlParcel` child); the arrival content (greeting, stock prim,
`SimulatorFeatures` over the Bevy CAPS path, a seed grant with
`EventQueueGet`); a chat `SlCommand` decoded grid-side; an `ObjectUpdate`
pushed with `with_sim` arriving as `ObjectAdded` and a `KillObject` as
`ObjectRemoved`; a CAPS `ParcelProperties` through the real long-poll
renaming the agent parcel; and a clean `LoggedOut`. A second test runs
two apps against two grids in one process. The tier is deliberately a
smoke test — behaviour belongs to the headless interaction/world tiers,
which stand their world up from `SlEvent` fixtures instead.

## Voice signalling

The stock scenario speaks **WebRTC voice**: `default_setup` enables the
`WebRtcStub` answerer on `SimSession::voice_mut()` and files the stock
parcel's estate-wide channel (its `channel_uri` is the region id, the
form Second Life sends) with the agent standing on it. The runtime
derives every backend advertisement from that — the login response's
`voice-config`, `SimulatorFeatures.VoiceServerType`, and a
`RequiredVoiceVersion` push over the event queue when the avatar
arrives — so a scenario that leaves voice disabled advertises none of
them, and one that sets a Vivox fixture instead (`set_vivox_account`)
advertises `vivox`. A client's `RequestVoiceAccount` (WebRTC offer) is
answered with a JSEP answer, its `SendVoiceSignaling` trickle is recorded
on the connection, `RequestParcelVoiceInfo` returns the region-id
channel, and a logout closes the session; the grid side sees
`VoiceProvisionRequested` / `VoiceSignalingReceived` /
`ParcelVoiceInfoRequested`. No media plane: nothing listens on the
advertised loopback candidate, so a real viewer will negotiate and then
sit in "connecting" — the signalling, not the audio, is what this
exercises. Chat-session channels can be gated with
`set_channel_credentials(channel, credentials)`.

The stock `Scenario` is intentionally small (an inventory skeleton, a library,
one parcel, one box, a chat greeting, WebRTC voice signalling). A real viewer
will ask for much more — terrain, appearance, textures — and renders a login
into a nearly empty world; growing the default scenario against what a viewer
actually requests is expected iteration, not a bug. Firestorm's seed-request
retries (up to 30×) are harmless: the grant is minted once, so every retry
gets a byte-identical reply.
