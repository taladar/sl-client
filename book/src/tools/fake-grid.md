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

## Session lifetime

A session lives exactly as long as its machine is open. Four tasks
attend it — the UDP pump, the timer, the teleport responder and a reaper
— and all four exit on the per-session closed watch the flush rule flips
(logout, inactivity, retirement after a teleport away, abandonment) or on
the grid's shutdown watch. The reaper is what removes the session from
the grid's table, so a logout frees its socket, its clone of the
scenario's assets and its terrain instead of holding them for the life of
the process, and `/sim/<n>/…` stops resolving. Until it does, a closed
session is skipped anyway when the grid looks for the circuit hosting an
agent: `SimSession` never resets its `agent_presence`, so a logged-out
circuit still reports itself the root agent, and after a relogin a lure
must not be handed the dead one.

The grid shuts down with its handle (`FakeGrid::shutdown`, also on
drop): the accept loop stops, every held `EventQueueGet` poll ends
immediately with its 502 re-poll answer rather than sitting out the
hold, an in-flight teleport stops waiting for an arrival, and each
connection is shut down gracefully — or dropped if it has not finished
within a second. Connections are bounded (256 at once, each of which
must send its request head within 15 s), so neither a wedged peer nor a
flood of them can pin tasks and file descriptors.

## Determinism: the seed and the clock

Two things would otherwise make one run of a scenario incomparable with
the next: the identifiers the grid mints and the instants it stamps its
machines with. Both are injectable.

`FakeGridBuilder::deterministic(seed)` replaces the identifier source
with a seeded xorshift stream, so session ids, secure session ids,
circuit codes, capability tokens, and defaulted agent and region ids all
come out the same, in the same order, for the same seed and the same
content. The minted uuids still carry the v4 version and variant bits, so
nothing downstream can tell them from random ones. `determinism.rs`
pins the property end to end: two scripted login-to-chat runs against
`deterministic(1)` mint the same identifiers — down to the tokens inside
the granted capability URLs — and decode the same grid-side event
sequence.

`FakeGridBuilder::clock(now)` replaces the clock. Every grid-side instant
— the `now` each `SimSession` entry point takes, the instant a session
machine is created at, the `EventQueueGet` hold deadline, the stamp a
scenario hook is handed — is drawn from one `Now`
(`Arc<dyn Fn() -> Instant + Send + Sync>`) held by the grid core and by
every live session; nothing in the crate calls `Instant::now()` behind
the builder's back. The default is `system_clock()`; a test that pauses
tokio's timer passes `tokio_clock()`, so the machines and the timer tasks
that fire their deadlines agree on what time it is. A test driving the
grid side stamps its own sends with `FakeAgent::now()` for the same
reason.

Both `SimHook` and `SimEventHook` take that instant as a parameter — a
hook that sends never has to reach for a clock of its own.

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
no `--account` it creates `Test User` / `password`. `--region` repeats
(`Name` or `Name@X,Y` in grid coordinates; the first is the start region,
unplaced ones are laid out eastwards from 1000,1000), and a viewer can
teleport between the regions from its map — see below.

### Named scenarios

`--scenario <name>` picks the scene every region shows, from the registry
in `fixtures::scenarios`. Two exist today: `stock` (the default — one
region-wide parcel, one scripted box, an arrival greeting) and
`catalogue` (the named prim catalogue: one prim per rendering feature,
plus an NPC, with every asset they reference served — see below).

A scene is *named* so that a harness photographing it can say which one
it photographed, and so the next scene is a registry entry rather than a
change to the harness. Each scene also names its **landmarks** — a name
and a region position for each thing worth aiming a camera at
(`NamedScenario::landmarks` / `landmark(name)`), which is what the binary
logs on startup:

```text
scenario "catalogue": the named prim catalogue: one prim per rendering feature …
landmark "catalogue-resident" at <104, 136, 25.95>
landmark "plain-box" at <108, 136, 25.5>
landmark "checker-box" at <112, 136, 25.5>
…
```

### The launcher

`scripts/fake-grid.sh` is the launcher a cross-check run starts from:

```sh
scripts/fake-grid.sh --port 9100 --scenario catalogue
```

Its scenario default is `catalogue`, not the binary's `stock`: a launcher
run is a cross-check or a hand-driven Firestorm session, and both want
the feature row. The banner names the scene it started, so the two
defaults never have to be remembered.

It builds the release binary, refuses a port something is already
listening on (a leftover grid would otherwise answer the readiness probe
and the viewer would log into last run's scene), waits until the grid
answers `get_grid_info` — the document Firestorm fetches before it will
show a login screen — and only then prints how to reach it:

```text
  fake grid ready on 127.0.0.1:9100, scenario "catalogue"

    this workspace's viewer   SL_LOGIN_URI=http://127.0.0.1:9100/
    Firestorm                 --grid 127.0.0.1:9100 --multiple
```

Three things about that are not guessable. The port is **fixed**, not
ephemeral, because both viewers of a run are configured before either
starts and Firestorm caches a grid in its grid manager between runs. The
host is the IPv4 literal and never `localhost`, which resolves to `::1`
first while this grid listens IPv4-only — a viewer told `localhost` fails
to connect for a reason that looks nothing like the cause. And Firestorm
wants `--grid <ipv4:port>`, not `--loginuri`: `CmdLineLoginURI` is dead
code in its OpenSim build, while an unknown `--grid` name is treated as a
host and resolved through `GET /get_grid_info`, which this grid serves.
Give it `FIRESTORM_X64_USER_DIR=<a fresh temp dir>` too, or the run shares
settings, cache, logs and the credential store with your real session.

### The cross-check runner

`sl-crosscheck` does all of the above unattended: it starts the grid, runs
both viewers against it in turn, and collects what each of them wrote.

```sh
cargo build --release -p sl-client-bevy-viewer
cargo run --release -p sl-crosscheck -- \
  --scenario catalogue --look-at mesh-cube --day-position 0.25 \
  --firestorm "${FIRESTORM_BUILD}/newview/packaged/firestorm"
```

The camera is aimed at a **landmark by name**, from `--look-from` metres
south and `--look-above` metres up — south because the fixture row runs
west to east, so a camera to the south sees the row rather than the end
of it. Without `--firestorm` (or `SL_CROSSCHECK_FIRESTORM`, which an
uncommitted `.env` beside the sources can set once) only this viewer
runs, and that is a **one-sided run as asked**, not a failure: the exit
status follows whether every viewer that was *asked* to run produced
frames.

A run leaves `run.json`, the two configuration files, and per viewer its
`frame_NNN.png` sequence, `scene.json` (when that viewer writes one),
`harness-status.json` and its own `viewer.log`.

The grid runs **inside** the runner rather than as a spawned
`sl-fake-grid`, which is the launcher's port lesson taken one step
further: a readiness probe proves a *port* answers, while binding the
port in-process makes "the grid that answered is not the one you started"
impossible rather than merely detectable.

Two invariants are worth knowing before reading a run:

- **A viewer is asked to quit, never killed.** The escalation is
  `SIGTERM` — which both viewers turn into a graceful logout — then the
  logout grace, then `SIGKILL`. A session the simulator still believes is
  logged in makes the *next* run fail to log in, and that failure looks
  exactly like a viewer bug. Firestorm's own `--quitafter` is unusable
  here for the same reason: it calls `forceQuit()`, which sends no
  `LogoutRequest`.
- **The status file, not the exit code, says whether a run happened.** A
  viewer that never got in world still writes a full set of frames, black
  and on schedule. Both viewers write `harness-status.json` before they
  log out, with the same five keys; no file means the run never reached
  that point. "The viewers differ" and "the run did not happen" are never
  reported the same way — and nothing in the runner says the viewers
  agree or differ at all, because nothing in it looks at a pixel.

Each viewer is confined to the run directory:
`FIRESTORM_X64_USER_DIR` for Firestorm, all four `XDG_*` roots for this
viewer. Not only the cache — this viewer rewrites its settings on the way
out, so a run would otherwise edit yours.

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
  baseline. Left `None` (the stock scenario does), the session serves the
  region's own ground — `RegionConfig::terrain.to_raw()`, so the download
  matches what the viewer is standing on. `flat_terrain_raw(height_m)`
  builds a flat 256 × 256 RAW32 file for a fixture that deliberately
  differs.

The stock scenario ships `motd.txt`, one scripted object
(`STOCK_SCRIPTED_OBJECT_LOCAL_ID`) whose task inventory holds a script with
a body, and a covenant. Behaviour the fixtures do
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
4. the region's **ground** — the 256 LAND patches as `LayerData` messages,
   then the WIND and CLOUD layers (see below);
5. one full **`ObjectUpdate`** carrying every fixture object.

The avatar goes first for a protocol reason: a `LayerData` message carries
no region handle, and the client labels each patch with the handle it
learned from that circuit's **first object update** — patches that arrive
before it are stamped with handle zero.

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

## Typed prim fixtures and the catalogue

`box_prim` makes an untextured cube and nothing else, because
`full_update_block` emits only the **raw byte fields** of an `Object`: its
`texture_entry`, `extra_params`, `particle_system` and `texture_anim`
travel as blobs, and the typed views beside them (`extra`, `particles`,
`texture_animation`) are what a *decoder* filled in — an encoder never
reads them. A fixture that wants a textured, lit, flexi or mesh prim has
to write those blobs itself.

`fixtures::PrimFixture` does. Each builder method sets a typed value and
`build()` packs all four blobs through `sl-proto`'s own
`encode_texture_entry` / `encode_extra_params` / `encode_particle_system`
/ `encode_texture_anim`, which are the exact inverses of the client's
decoders — so a test asserts the fields it seeded:

```text
PrimFixture::boxed(local_id, full_id, owner, position, scale)
    .shape(..)              // path/profile curves: sphere, cylinder, tube
    .textured(key)          // one texture on every face
    .face(i, &FaceStyle { texture, color, alpha, glow, fullbright, shiny,
                          bump, repeats, offset, rotation, material, media })
    .mesh(key, faces)       // ExtraParams sculpt block, LL_SCULPT_TYPE_MESH
    .sculpt(map, kind)      // .. or a sculpt map with its stitch kind
    .pbr(face, material)    // ExtraParams RenderMaterial (GLTF)
    .light(..) .projector(..) .flexi(..) .reflection_probe(..)
    .particles(..) .texture_anim(..) .hover_text(..) .media_url(..)
    .rotated(..) .child_of(parent, offset, rotation)     // a linkset child
    .attached_to(wearer, point, item, offset, rotation)
    .build()
```

`linkset(root, children)` re-parents every child to the root and returns
the objects root-first — a linkset is one object per prim on the wire,
linked only by the shared `parent_id`. An attachment's point rides in the
`state` byte with its nibbles swapped (`attachment_state_from_point`, the
inverse of the viewer's `ATTACHMENT_ID_FROM_STATE`) and its item id in an
`AttachItemID` name-value.

Quantization is visible here: the flexi block's floats travel as a byte
each, so the typed `extra` a fixture holds and the `extra` a client
decodes agree only to the wire's resolution. Compare against the decoded
**blob**, not the typed value — the wire is the contract.

`fixtures::RegionFixture` is one region's whole content as a value —
`world`, `assets`, `materials`, `media`, `environment`, `terrain` — and
`into_region(base)` is the single place that knows which surface serves
which piece (objects and parcels over UDP, assets over
`GetTexture`/`GetMesh2`/`ViewerAsset`, materials over `RenderMaterials`,
media over `ObjectMedia`, the environment over `ExtEnvironment`, the
ground as `LayerData` plus the estate RAW download).

`fixtures::catalogue()` is the **named catalogue**: sixteen prims, one per
rendering feature, in a west-to-east row 8 m north of the arrival point at
4 m spacing, with every texture, sculpt map, mesh and material they
reference served. `catalogue::entries()` / `entry(name)` give a subject's
id and position, so a check finds "the mesh prim" by name rather than by
a hard-coded local id, and the same fixture backs the automated tiers and
the binary's `--scenario catalogue` — which is what makes a Firestorm
session and a full-stack capture look at the same objects.

The procedural assets it needs come from `sl-test-assets`:
`RgbaImage::checker` / `solid` (as JPEG2000), `sculpt_sphere` (a sculpt
map — geometry stored as a texture), `mesh::unit_cube_mesh_asset` (the
LLSD-binary header plus zlib-compressed LOD blocks `sl-mesh` decodes) and
`gltf_material_asset` (the `AT_MATERIAL` LLSD envelope around a glTF 2.0
document).

### Fixture textures: size it honestly, and mind the cache

Two things about fixture textures cost a live-debugging session each.

**Size them like real content.** `TEXTURE_SIZE` is 512 — what a Second Life
diffuse texture is — and the NPC bakes are 512 as well. A 64² fixture
texture is sharp in a decode test and renders as a stuck low-LOD blur: a
one metre prim face at conversational range covers several hundred screen
pixels, the pixel-area LOD driver asks for discard 0, and there is nothing
finer to fetch. The encoded cost of the honest size is about 13 kB for the
checker and ~300 bytes for a solid at *any* size, so there is nothing to
save. A **sculpt map** is the exception (`SCULPT_MAP_SIZE`): it is geometry,
one vertex per texel, and the reference viewer reads at most a 64² grid.

**A texture's identity is its UUID, not its bytes.** Change a fixture
texture's *content* under a stable id and every viewer that already fetched
it keeps rendering the old pixels from its disk cache — including a run
under a different avatar, because the texture cache is not per-account. An
A/B against a viewer therefore has to start from a cold cache: point
`XDG_CACHE_HOME` at a scratch directory for the run (better than deleting
the real cache, and it isolates the whole account tree). The give-away in
the log is the LOD driver's own line, which prints the size it learned:

```text
texture …ca70001 pixel-area LOD: discard 2 -> 0 (area 196888 px, native 64x64)
```

`native 64x64` for a texture the grid is serving at 512² means the viewer
never re-fetched it. Run the viewer with
`RUST_LOG=warn,sl_viewer_world_objects=debug` to see those lines.

## NPCs: other avatars as content

The grid rezzes only the arriving agent's own avatar and has no
inter-session broadcast, so a second logged-in avatar is invisible to the
first. Everything a viewer does with *other* people — the body, the bakes,
the name tag, the playing animation, the attachment that follows a wearer
— is therefore scripted content: an `fixtures::NpcFixture` on
`SceneFixtures::npcs`.

```text
NpcFixture::new(local_id, AvatarIdentity::new(agent, "First", "Last"), position)
    .looking(NpcAppearance::solid(agent, colour))   // .. or ::default_avatar()
    .rotated(rotation)
    .animating(animation)
    .wearing(PrimFixture::boxed(..), point, item, offset, rotation)
```

What reaches the wire per NPC, appended to the arrival burst in the order
a simulator introduces one: the **avatar objects** (`world::avatar_prim`
— the same `LEGACY_AVATAR` body the arriving agent is rezzed as, carrying
the `FirstName` / `LastName` name-values), then each one's
**`AvatarAppearance`**, then its **`AvatarAnimation`**, then the
**attachments** (ordinary child objects whose parent is the NPC's
region-local id and whose state byte carries the attachment point). The
bodies precede the appearances because an appearance names an avatar the
client has to already know, and the attachments come last because each
names its wearer. `SceneFixtures::all_objects` folds the NPCs' objects in
beside the prims, so an object refetch answers for them too.

The three server-side pushes are `SimSession::send_avatar_appearance`,
`send_avatar_animation` and `send_terse_update` (the every-frame motion
message, for a scripted move). One detail is worth knowing: an
`AvatarAnimation`'s `AnimationSourceList` is positionally correlated with
its animation list, and an animation with no triggering object is stamped
with the **avatar's own id**, not a nil one — what OpenSim's
`SendAnimations` does, so a receiver never sees a nil source.

The shape is `NpcAppearance::DEFAULT_VISUAL_PARAMS`: OpenSim's own
`AvatarAppearance.SetDefaultParams` table, the 218-byte "Ruth" body a grid
hands an account with no stored appearance. Do not reach for the
obvious-looking midpoint of each param's range instead — it renders a
badly distorted avatar, because the ranges are not centred on anything a
body wants to be. A receiver reads the vector positionally against its own
transmitted param list, which in the standard `avatar_lad.xml` is 253
params: exactly those 218 classic ones (every id below 10000), then the 33
physics params and two more, so OpenSim's vector lands slot for slot and
the rest falls back to each param's default.

A bake is served like any other texture. `NpcAppearance::solid` paints one
solid per body-region baked slot (head, upper, lower) under ids derived
from the agent id (`ba4e<slot>-…` plus the avatar's low 96 bits, so two
NPCs never share a bake), the texture entry names them in their
`avatar_texture` slots, and `RegionFixture::into_scenario` registers the
bytes — the OpenSim path, where no server-bake service is advertised and
the viewer fetches each bake with a plain `GetTexture`.

The catalogue's own NPC (`catalogue::npc()`) stands one slot west of the
prim row, baked blue, playing the built-in `stand` animation and wearing a
checker box on its skull. The animation *asset* is not served — nothing in
the fake grid serves animations yet — so a viewer records it as playing
and falls back to its own idle.

## The ground: terrain, wind and clouds

A region's ground is not scenario content but region content, so it lives
on `RegionConfig::terrain` (a `TerrainFixture`) rather than on `Scenario`.
It is the one source three different paths read:

- **`to_patches(handle)`** — the 256 LAND patches (16 × 16 metre cells
  each) the arrival burst streams. `SimSession::send_terrain` walks them
  in OpenSim's spiral order (`SendLayerTopRight` / `SendLayerBottomLeft`:
  the outer ring from the south-west corner, then the next ring in) and
  packs at most `TERRAIN_PATCHES_PER_MESSAGE` into each `LayerData`
  message, so the region fills from its edges inwards.
- **`wind_patches` / `cloud_patches`** — the wind field as the *two*
  patches OpenSim's `SendWindData` packs into one message (the east then
  the north velocity component of one whole-region 16 × 16 field, both at
  patch position `(0, 0)`), and the cloud field as one. Both go out
  through `SimSession::send_layer_data`, which sends exactly one message:
  `send_terrain` addresses patches by grid position and would collapse the
  wind layer's two.
- **`to_raw()`** — the same heights as the estate RAW32 download, so
  "download terrain" and the rendered ground agree. The height multiplier
  is the finest one whose range still covers the field.

`Heightfield` is the shape: `Flat`, `Slope` (west to east), `Ridge` (a
crest along the region's centre line) or `Steps` (flat terraces, so every
height is exact — what a ground-snapping or foot-IK check wants).
`composition` carries the four detail texture ids and their per-corner
blend heights, and is what the region's `RegionHandshake` announces; the
stock scenario registers a JPEG2000 solid for each of the four default
Linden ids (`scenario::default_assets`, from `sl-test-assets`) so the
ground shades against real textures instead of four failed fetches.

`default_assets` carries the rest of the **library** a viewer asks any
grid for before it has been told about a single fixture: the built-in sun
and moon discs, the cloud noise, the rainbow and halo overlays, the star
bloom, the wave normal map and the blank plywood every untextured prim
face falls back to
(`sl_proto::BUILTIN_ENVIRONMENT_TEXTURES` plus
`sl_proto::DEFAULT_PRIM_TEXTURE`; the pixels come from
`sl_test_assets::builtin`). No viewer ships these — Firestorm marks the
sky ones `// dataserver` — so without them an arrival is eight fetches
that each burn a full retry budget, and the sky draws no sun at all.
They are stand-ins rather than Linden's own pixels, shaped to be
recognisable in the role: a disc reads as a sun, and the halo's bright
band sits at the 22° radius the shader samples it at.

`RegionConfig::environment` is the region's other environmental half: an
`EnvironmentSettings` (day cycle, day length, sky-track altitudes) served
by the `ExtEnvironment` capability. Left `None`, the session's stock
four-hour day answers.

## Teleporting between regions

A `SimSession` has its region handle fixed at construction, so a teleport
is always a **second session**: a fresh loopback socket, `SimSession` and
`SimCaps` in the destination region, seeded with that region's scenario
under the login's identity (the client opens every circuit with its login
`UseCircuitCode` triple). `teleport.rs` sequences it the way OpenSim's
`EntityTransferModule` does:

1. `TeleportStart` and the progress keys on the source (`resolving`, then
   `sending_dest` / `sending_home` / `sending_landmark`, then `arriving`
   — the keys of Firestorm's `teleport_strings.xml`, which the viewer
   localises; `sl_proto::teleport_strings` holds them);
2. the destination session is prepared, placed (`set_arrival_position`
   — the `AgentMovementComplete` lands the avatar where the request
   asked) and **registered before it is announced**, because the client
   POSTs the destination seed the moment `EstablishAgentCommunication`
   arrives and an unregistered `/sim/<n>/…` answers 404;
3. the event-queue trio on the source: `EnableSimulator` (the client
   opens a child circuit), `EstablishAgentCommunication` (the seed), and
   `TeleportFinish` — the full reference record (`TeleportFinishInfo`:
   agent id, region handle, region size, …; Firestorm builds the
   destination region object from the handle, and the client reports the
   wire handle rather than the one it requested, which is what a lure or
   landmark teleport needs);
4. once the destination sees `AgentArrived`, the source is retired:
   `DisableSimulator` to the client, the session closed
   (`ServerEvent::CircuitRetired`, the pumps exit on the per-session
   closed watch), its CAPS paths forgotten, and a `TeleportNotice` on
   `FakeGrid::teleports()`. No arrival within `TELEPORT_ARRIVAL_TIMEOUT`
   fails the teleport with `timeout_tport` and abandons the destination.

Two entry points share the sequence. The **responder task** every session
runs answers the client's own requests: `TeleportLocationRequest` by
handle, `TeleportLandmarkRequest` through the landmark asset in the
scenario's asset store (`sl_wire::parse_landmark`, both on-wire versions;
resolved by region id, so give a `RegionConfig` a fixed `region_id` for a
landmark fixture; `None` = home = the account's start region),
`TeleportLureRequest` through the OpenSim lure-id convention (a
`FakeParcelId`: handle + position packed into the UUID; an opaque id is
taken as the offering agent's id). A request that resolves nowhere is
refused with the matching failure key (`invalid_tport`,
`nolandmark_tport`, `no_host`), so the viewer's teleport screen never
hangs; a same-region request finishes as a `TeleportLocal`. The explicit
`FakeGrid::teleport_agent(&agent, "Region", position, look_at)` is the
grid-initiated counterpart (what `llTeleportAgent` or a scripted push
does — no client request at all; the client follows a remote
`TeleportStart` exactly as the reference viewer does) and hands back the
destination `FakeAgent`.

The real-client tests in `tests/client_end_to_end.rs` cover each path;
with the binary, `sl-repl-tokio`'s `teleport <handle> <x,y,z>` (handle =
`grid_x*256 << 32 | grid_y*256`) shows the whole sequence as events.

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
two apps against two grids in one process. This file is the *plumbing*
smoke: it proves the socket, retransmission and CAPS paths the headless
tiers bypass. It is not the whole full-stack tier — anything whose failure
needs grid **sequencing** (arrival ordering, CAPS fetch paths, teleport
and crossing hand-overs, `KillObject` timing, multi-region offsets,
in-flight asset leaks, NPC appearance delivery) belongs in the viewer's
full-stack harness against this grid, read back as pixels; reaction logic
that a fixture world can stand up from `SlEvent`s belongs in the
interaction tier. See the *viewer test harness* chapter.

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

The stock `Scenario` is intentionally small (an inventory skeleton, a library
of the twelve textures above, one parcel, one box, a chat greeting, WebRTC
voice signalling). A real viewer
will ask for much more — terrain, appearance, textures — and renders a login
into a nearly empty world; growing the default scenario against what a viewer
actually requests is expected iteration, not a bug. Firestorm's seed-request
retries (up to 30×) are harmless: the grant is minted once, so every retry
gets a byte-identical reply.
