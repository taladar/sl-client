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

A session lives exactly as long as its machine is open. Five tasks
attend it — the UDP pump, the timer, the teleport responder, the
neighbour announcer and a reaper — and all five exit on the per-session
closed watch the flush rule flips
(logout, inactivity, retirement after a teleport away, abandonment) or on
the grid's shutdown watch. The reaper is what removes the session from
the grid's table, so a logout frees its socket, its world fixtures and
its terrain instead of holding them for the life of the process, and
`/sim/<n>/…` stops resolving. Until it does, a closed
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
in `fixtures::scenarios`. Three exist today: `stock` (the default — one
region-wide parcel, one scripted box, an arrival greeting), `catalogue`
(the named prim catalogue: one prim per rendering feature, plus two NPCs
— one standing, one sitting on a bench — with every asset they reference
served — see below), and `border` (one checkered marker pillar floating
just inside the region's west edge, which with two adjacent `--region`s
is a scene for looking across — and walking over — a border).

A scene may also say how it dresses a **pair** of regions
(`NamedScenario::pair`), which is not the same as two copies of it: the
two halves of a border are not interchangeable, so the registry names the
near one (the west, where the agent logs in) and the far one separately.
`border` is the only scene with a second half today, and a harness asked
for a pair from a scene that has none is told so rather than handed two
copies.

A scene is *named* so that a harness photographing it can say which one
it photographed, and so the next scene is a registry entry rather than a
change to the harness. Each scene also names its **landmarks** — a name
and a region position for each thing worth aiming a camera at
(`NamedScenario::landmarks` / `landmark(name)`), which is what the binary
logs on startup:

```text
scenario "catalogue": the named prim catalogue: one prim per rendering feature …
landmark "seated-resident" at <100, 136, 25.8>
landmark "sit-bench" at <100, 136, 25.25>
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

**Choosing the sun.** `--day-position <0..1>` pins the time of day in
both viewers, and any comparison involving light wants it: the two
viewers' *own* defaults are not the same sky.

Both pin the same way — sample the **region's** day cycle at that
position, hold the result as a fixed local sky — which means the region
has to serve a cycle the position can choose between. A region's stock
environment is deliberately a single keyframe (that is what makes two
captures minutes apart comparable), so `--day-position` also dresses
every region of the run with the four legacy WindLight presets, keyed
midnight / sunrise / midday / sunset at `0.0` / `0.25` / `0.5` / `0.75`.
A scene that carries its own multi-frame cycle through
`RegionFixture::environment` keeps it, frames and all.

Done on the grid rather than in a viewer deliberately. Either viewer
could synthesise a cycle of its own when the region's cannot be sampled
— and then the two would be photographing two different skies while both
reporting the request as honoured, which is the one failure a
cross-check must not be able to produce.

Each viewer reports what its pin selected in `harness-status.json`:

```json
{
  "ok": true,
  "reason": "complete",
  "frames_written": 30,
  "frames_expected": 30,
  "viewer": "firestorm",
  "day_position": {
    "requested": 0.25,
    "honoured": true,
    "detail": "blended the region's day cycle (4 sky keyframes) at 0.25"
  }
}
```

An **unhonoured** pin fails that viewer's status, fails the run, and is
printed — `SUN NOT PINNED at 0.25 — …`. A capture lit by a sky nobody
chose is indistinguishable, from the outside, from a good one: it is a
directory of plausible frames of the wrong scene. So is a viewer that
says nothing about the pin at all (`SUN NOT REPORTED`), which is what a
build older than the field looks like.

**Two regions, and walking between them.** `--neighbour` stands the
scene's *second* half one slot east of the first and lets the grid
announce it, and `--cross-after <seconds>` walks the agent over that
border once it has arrived (a one-step `Timeline` carrying
`Action::CrossRegion`, so the crossing is the grid's, not a second
login). `--cross-after` implies `--neighbour`.

Only a scene that says how it dresses **both** halves can do this, which
today is `border` alone — a pair is not two copies of a one-region scene,
and asking for one from a scene that has no second half is refused rather
than obliged. The two halves are not interchangeable: the near one is the
west, with its vehicle against its east edge, and the far one is the east.

The crossing is timed from the agent's *arrival*, while the capture
starts when the scene settles, so a `--cross-after` that lands before the
first frame photographs only the aftermath. Give it more than
`--settle-timeout` and enough `--frames × --interval` to still be running
when it fires.

One thing a crossing run must not do is pin the camera. The reference
harness re-derives the camera pose from the agent's **current** region
every frame (`applyCamera` → `getPosGlobalFromRegion`), so a
region-local pose that framed the border before the crossing frames the
*next* border after it — the shot jumps a region east at the moment of
interest. Leaving `--look-at` off puts the camera back on the avatar,
which is where a crossing wants it anyway.

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

### Reading a run: the report

`sl-crosscheck-report` is the other half. It reads a collected run and
writes `<run>/report`: a **contact sheet**, a **difference image** per
frame, and `report.txt` / `report.json`.

```sh
cargo run --release -p sl-crosscheck --bin sl-crosscheck-report -- \
  crosscheck-runs/catalogue
```

Three outputs, in increasing order of how often they identify the bug:

- the **contact sheet** — both viewers' frames tiled, one row per capture
  index and one column per viewer, each cell naming its viewer and frame
  and the sheet naming the scene and camera. Frames are spread across the
  whole run rather than taken from its start (a scene is still rezzing at
  the start), and it says how many it left out.
- the **image diff** — per pair, the mean absolute channel difference,
  the fraction of pixels differing by more than 8/255, windowed SSIM on
  luma, and the worst 32-pixel tiles. Three numbers because no one of
  them is enough: a uniform exposure difference moves the mean and leaves
  SSIM alone, and the tiles are what separate "everything is slightly
  darker" from "one object is wrong".
- the **scene-dump diff** — the field-by-field comparison of the two
  `scene.json` documents, and the output that names a cause: a texture id
  that differs, a level of detail that differs, an object one viewer
  never built.

Expect a **large baseline difference** and read it as one: the two
viewers do not share a renderer, so tone mapping, exposure, shadow
filtering and anti-aliasing differ everywhere at once. The signal is a
change in the difference between runs, or a difference localised to one
tile. Nothing here fails a build or enters `cargo nextest`, for the same
reason this workspace has no golden images.

Three things the scene diff will not do, each because doing them
manufactures findings:

- **It matches on grid ids only.** The reference models its terrain
  patches, sky, water and clouds as objects with `local_id` 0 and an
  `app-…` class — 275 of its 296 objects in the catalogue scene — which
  this viewer does not model as objects at all; they are counted as
  scenery, not reported as missing. Control avatars have no grid
  identity, so they are paired by position rather than by id.
- **It annotates the known semantic differences instead of ranking
  them.** `num_faces` is what this viewer drew against what the reference
  declares; `is_flexible` is "declares itself flexible" here against "is
  being drawn flexible" there; `camera.aspect` is an artefact of how the
  reference takes its snapshot; an animation's `loop_time` is where each
  viewer's clock had reached, so two frames of one loop are two phases,
  not two viewers disagreeing. The default motions the reference starts
  on every avatar — head rotation, eye, body noise, breathing, physics,
  hand pose, pelvis fix — are named as adjusters this viewer implements
  differently, not as animations it failed to play.
- **It prints the render, camera and environment settings above the
  findings.** The first thing the first live pair of dumps found was not
  in the images at all: `mesh_lod_boost` 1.0 against 2.0 and a draw
  distance of 512 m against 128 m, which *explain* the `lod` differences
  below them.

A half whose `harness-status.json` is missing is a run that did not
happen, and **nothing is diffed against it** — neither its frames, which
are black and on schedule, nor its scene dump, which describes an empty
world. Point the tool at several run directories to rank the scenes
against each other by median frame difference.

And "the reference viewer is right" is a prior, not evidence. One
difference here looked exactly like a bug of ours and was upstream Linden
code: Firestorm drew every avatar with no right hand, because
`avatarSkinV.glsl` reads one past the end of its matrix palette for
`mWristRight` and `NaN * 0` is `NaN` ([secondlife/viewer#6240]).

[secondlife/viewer#6240]: https://github.com/secondlife/viewer/issues/6240

### Calibration: what these fixtures look like in the reference viewer

A different use of the same runs, and the reason to keep them apart: the
report above says whether the two viewers *differ*, while this says what a
fixture **is supposed to look like**. Those answers are sentences, not
images — an oracle calibrated against a stored screenshot is calibrated
against one machine's driver — so they are written down here rather than
committed as pixels. Measured on 2026-09-10, Firestorm 7.2.5, at
1920×1080 and the reference's own 60° lens.

**The ground reads as one green plain, and the region has a visible
edge.** The stock region is flat at 25 m, so its four detail solids never
separate by height; what the reference draws instead is a mid-green
mottled with pale grey and faint brown, because its terrain shader mixes
the four by a noise field as well as by altitude. There are no patch
seams. The region's own **edge** is unmistakable — the green plain stops
along a straight line and the void beyond it is a dark blue-grey band at
the water height, with the sky above that. So "the ground rezzed" and
"the region is 256 m across" are both readable by eye, and *the colour of
the ground is not a height reading*: do not calibrate an oracle that says
"green means low".

**The sun is nearly overhead, and the stars are out anyway.** The stock
environment is one keyframe carrying the reference's own default sky, so
every capture of it is lit identically — that is the point of it. The
default sky puts the sun at **80° altitude**, so the ground is lit from
almost straight above (a plywood cube shows a bright top face and nearly
black sides) — and it also carries `star_brightness` 250, so a
midday-lit ground sits under a blue sky with stars in it. Read that as
the fixture's signature rather than as dusk, and do not infer the time of
day from the sky. That is the sky of an **unpinned** run; a run that
passes `--day-position` is photographing a different cycle on purpose —
see below.

**The checker is legible at the fixture's own distance.** The catalogue's
prims are 1 m and its checker is 512² with 128 px cells, so a face is
4 × 4 cells of 25 cm. From the runner's default framing — 8 m south,
2 m up — that face lands about 130 px wide, so a cell is roughly 30 px:
not a texture you have to squint at, and enough that a wrong repeat count
or a stuck low LOD is obvious rather than arguable. This is what
`TEXTURE_SIZE = 512` buys; the same fixture at 64² reads as a blur at
this distance and nothing about the framing would tell you why.

**The border is visible only because the grounds are painted.** Two
regions meeting share no geometry a viewer draws: nothing marks the line,
no seam, no edge, no change in shading. What makes it legible in the pair
scene is that each side's ground is its own flat colour, and then it is
about as legible as a picture gets — a hard vertical boundary between a
blue half and a yellow half, straight down the frame. An oracle for "am I
looking at the border" is therefore a colour test, never an edge test.

**The neighbour is drawn before you get there.** Standing in the west
region, the east region's ground is already on the horizon as a yellow
band — the neighbour announcement opened its child circuit and it has
been streaming since arrival. The pair scene's pillars are 4 m inside
each region's *west* edge, so from a camera at the shared line you see
the far region's pillar and not the near one's; the near region's stands
252 m behind you, beyond the draw distance.

**A crossing changes everything at once, in one frame.** With frames
1 s apart, the last west frame and the first east frame are adjacent:
whole-ground blue in one, whole-ground yellow in the next, with no
intermediate. The reference logs `process_crossed_region()` and
`Entering region [Fake Region East]`, and does not tear the scene down.

**Two avatars survive it, not three.** After the crossing the reference
holds the agent's own body and **one** rider — and the pair scene has a
rider on each side. That is not a loss: both riders are the same
`RIDER_AGENT`, and an avatar is keyed by agent id, so the two are one
avatar that belongs to whichever region streamed it last. The same goes
for the vehicle: one grid-wide id, two regions, one object — which is
exactly the property a ridden crossing needs and exactly why the marker
**pillars** were given an id each instead. After the crossing the
reference reports that one vehicle under the destination's own local id
(`0x340`, not the source's `0x310`) at the destination's own position,
which is the handover renumbering seen from the other end.

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
  login response's `map-server-url` points at the login URI on either
  flavour (and an OpenSim-flavoured grid's `SimulatorFeatures`
  `OpenSimExtras` says it a second time), so a viewer's world map
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

## The world map

The tiles above are only half of a world map; the other half is the UDP
catalogue drawn under them, and `world_map.rs` answers it. The region table
never changes after start-up, so the catalogue — one `MapRegionInfo` per
configured region — is built once and shared by every session, which is right
in more than one sense: a viewer opening its map asks *whichever* simulator it
happens to be on about the whole grid, so every session has to be able to
answer for every region.

- **`MapBlockRequest`** — every region whose grid coordinates fall inside the
  requested rectangle.
- **`MapNameRequest`** — every region whose name starts with the search text,
  case-insensitively, because the viewer's search box sends whatever has been
  typed so far.
- **`MapItemRequest`** — for `AgentLocations` on the session's own region, one
  green dot at the agent's position; anything else answers with an empty reply
  of the requested type rather than silence, which is what a viewer's "no
  events here" needs to see.
- **`MapLayerRequest`** — one layer covering the bounding rectangle of every
  configured region.

A block's `map_image_id` is the **region id**, which is what OpenSim reports for
a region with no separately-uploaded map asset. It is not a texture the grid
serves: the tiles go out over HTTP, as they do on every modern grid.

This is what lets a client *find* somewhere to go — `sl-conformance`'s
`teleport-cross-region` discovers its destination through a map query, exactly
as `sl-survey` enumerates a real grid.

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
- **The estate covenant notecard** (`estate_covenant`) — the one
  `TransferRequest` source that is still a fixture, because it belongs to
  no inventory item: it is addressed by estate asset type, not by an asset
  id. A miss is refused with `UnknownSource`, which the client surfaces as
  `TransferFailed` instead of hanging.
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

Task inventories used to live here too. They do not any more: a contents
*serial* only means anything if the store that answers it is the store a
write advances, so the listings moved to the region's world (below), where
an `UpdateTaskInventory` lands. Their **bodies** followed, for the same
reason one step further out. They were a `(task, item)` map stated up
front, which no fixture could extend: an item dropped into a prim is
minted a fresh task item id, so its bytes could never have been stated,
and the `TransferRequest` for it was refused — the one item whose contents
serial a test had just watched advance was the one item whose asset could
not be read back. A task item now resolves the way every other asset fetch
does: through the item's own `asset_id`, against the one grid-wide store
(`udp_assets::task_item_asset`). The request's own `asset_id` field is not
trusted, which is what makes a save observable — the fetch after one
returns the new bytes because the item now names them.

The stock scenario ships `motd.txt` and a covenant. Behaviour the fixtures do
not cover goes in `Scenario::on_event`, a hook that sees every drained
`ServerEvent` with the live `SimSession` (after the stock behaviour ran).
`client_end_to_end.rs` drives each of these flows through the real
`sl-client-tokio` commands.

## The world fixtures

`Scenario::world` (`SceneFixtures`) holds the region's parcels
(`ParcelInfo`), objects (`Object`) and per-object task inventories — the
records the client decodes, so a test asserts exactly what it seeded. The
scenario states what the region *starts* as; what it has *become* lives on
the `RegionEntry` (`RegionWorld`, one `SceneFixtures` behind one lock),
shared by every session in that region rather than cloned into each. Two
regions never share one, which is what a handover needs: the region an
object left and the region it arrived in disagree for a moment by design.

A real simulator pushes a burst of
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
`STOCK_SCRIPTED_OBJECT_POSITION`, holding the stock script item in its task
inventory — so the listing describes an object a viewer can actually see
and click, and the two are stated in one place.

### The write path

Everything above is content the region was *handed*. Three client messages
change what it holds, and all three are answered against the region world:

- **`ObjectAdd` → `ServerEvent::RezObject`.** The simulator mints the
  object's region-local id (`SceneFixtures::mint_local_id`, always above
  every id in use *and* every id ever minted, so a rez after a derez cannot
  reuse a handle a viewer is still keyed on) and its full key, builds the
  prim from the client's `PrimShape` (`prim_from_shape`), adds it to the
  region and streams it straight back — the rezzing client cannot use the
  object until it learns the ids it did not choose.
- **`DeRezObject` → `ServerEvent::DerezObjects`.** The destination decides
  both halves and nothing else does: `DeRezDestination::agent_folder` names
  the folder an inventory item is minted in (announced the way the imitated
  grid announces one — see "How this grid does inventory" — and filed into
  the session's own `SimInventoryTree` so a later `UpdateTaskInventory` can
  resolve it), and
  `removes_from_world` says whether the world copy then goes (answered with
  a `KillObject`). A destination that does neither gets a `DeRezAck`. The
  split follows OpenSim's own `Scene.DeRezObjects`, whose
  `takeCopyGroups` / `takeDeleteGroups` lists are exactly these two
  predicates. An id the region does not have is killed on the client
  anyway, so the two agree again. A take also *writes* the object down —
  see "A taken object's asset" below for where those bytes go, which is
  the one place the fake grid has to pick a live grid to be.
- **`RezObject` from an item → `ServerEvent::RezObjectFromInventory`.** The
  other half of a take. The item is resolved by id out of the agent's own
  inventory (the masks and CRC the client sends are what the *viewer*
  believes, and OpenSim does not check them either), its object body is
  decoded, and the region mints an id per prim — so a linkset comes back
  whole, children re-parented to the root's new region-local id. The root
  lands at the ray's end point; a child keeps its stored offset. The item
  survives unless it is no-copy, which is OpenSim's rule
  (`DoPostRezWhenFromItem`) and is decided by the item's own owner mask
  rather than by the client's `remove_item` flag.
- **`UpdateTaskInventory` → `ServerEvent::UpdateTaskInventory`.** The item
  is resolved **by id from the agent's own inventory**, not trusted from
  the copy the client sent, minted a fresh task item id (a task copy is a
  new item that happens to name the same asset), and written in — which
  advances the object's contents serial. The listing a following
  `RequestTaskInventory` serves is re-generated from the live store, so it
  can never disagree with the serial that announced it.

Because the world is the region's, one avatar's rez is a change every
avatar in the region sees. There is no simulation loop to sweep for it, so
the session that made the change publishes a `RegionUpdate` and a
per-session `run_region_watcher` task turns it back into the message its own
circuit needs — an `ObjectUpdate`, a `KillObject`, or one of the three
subscription pushes below — skipping the changes its own session published,
which were sent directly. A watcher that falls behind
its broadcast logs a warning rather than swallowing it: a lost `KillObject`
is a ghost object standing in that viewer until its next refetch.

`sl-conformance`'s `task-inventory` case runs the whole of this offline —
rez a container, rez and take a donor, drop it in, watch the serial
advance, read the listing back over Xfer, trash the container.

### A taken object's asset

The two live grids disagree about `AssetType::Object`, and the disagreement
is total, so the fake grid says which of them it is being rather than
picking whichever was easier to build.

Measured on aditi 2026-09-06 by the `object-asset-format` conformance case:
**Second Life gives a viewer no asset id for an object inventory item.**
Eleven of eleven object items answered with a nil `asset_id`, in the AIS3
folder listing and again in the per-item `GET /item/<id>`, and all eleven
were full-perm to their owner — so it is not the familiar "no asset id
unless you fully own it" rule, it is the class. OpenSim is the opposite:
every object item names an asset, and `ViewerAsset` serves it as
`SceneObjectSerializer` XML.

`assets::ObjectAssetPolicy` picks a side and
`FakeGridBuilder::object_assets` sets it. The **default is Second Life**
(`Withheld`), because that is the grid this workspace targets and because
it is the configuration that *fails* a viewer which has come to rely on
opening a taken object's asset. A take then files its item under a nil
asset id and the object's body goes into a second store inside
`GridAssets`, keyed by the **item** id, that no capability reads. The two
stores share no keyspace, so the body is unfetchable by construction rather
than by an id nobody can guess. Ask for `ObjectAssetPolicy::Served` to get
OpenSim's side, where the item names a minted asset id and the grid serves
the body under it.

The take reads the policy once, in `world::taken_item`; `store_taken_asset`
then reads the rule back off the *item* — a nil id means the withheld store
— so the two halves cannot disagree about where a body went. And
`rez_from_inventory` asks the item first and the store second, which is why
**a rez works under both**: on Second Life too a taken object drags back out
of inventory, because the simulator resolves the body itself and the viewer
never needed it. The divergence is about what a viewer may *fetch*, not what
a resident may *do*.

One thing the switch does not govern: the seeded `Fixture Object` keeps its
asset id and stays fetchable either way. It is the fake grid's own fixture,
seeded so `asset-round-trip` has an authored object body to read back, and
no live grid has an item like it at all.

### Each grid's body is its own format

`AssetType::Object` is **two formats on the two grids**, so which body a take
publishes follows from which side the policy picked.

A **withheld** body is the Linden text form (`sl-object-asset`), which is what
Second Life is known to have written and which has **no keyword** for a face's
glow or material id, for `ExtraParams` (flexi, light, sculpt, mesh, light
image, extended mesh, render material, reflection probe), for floating text, a
media URL, a texture animation or a particle system. That list is asserted, not
described: `sl_object_asset::bridge`'s
`the_text_carries_none_of_the_modern_prim` sets every one on a live object and
watches it come back empty. No keyword for them will be invented, because
there is nothing left to check a guess against — Second Life exposes no
object asset to capture, and OpenSim writes XML instead — so a guess would be
both unfalsifiable and unreadable.

A **served** body is that XML: `<SceneObjectGroup>`
(`sl_object_asset::opensim`), transcribed from
`SceneObjectSerializer.ToOriginalXmlFormat` both ways. It loses none of the
list above — its `Shape` block carries the wire's packed `TextureEntry` and
`ExtraParams` blobs byte for byte, and floating text, media, a texture
animation and a particle system each have an element — and
`the_xml_carries_the_whole_modern_prim` is that same missing-field test read
the other way round. Writing the text under `Served` would name OpenSim and
serve bytes no OpenSim has ever produced, which is the one thing about that
policy that used to be unfaithful.

The take also writes each prim's **contents** into the served body, gathered
out of the region's task inventories while it still has the object: an object
update carries none, so a body that stated none would file a scripted prim as
an empty one.

A rez reads the format off the **bytes**, not off the policy — a
`<SceneObjectGroup>` opens with `<` and a text asset with the `{` of its first
prim header — so a grid can rez a body written under the other flavour instead
of failing on it.

### The body is a publication, not a store

A grid must not *rez* out of the text, and neither live grid does: OpenSim's
XML carries the whole prim, and Second Life's simulator has the object and
reads no asset. The fake grid keeps **the linkset a take removed** — a third
store in `GridAssets`, keyed by item under both policies — and
`rez_from_inventory` puts that back, minting fresh ids for it exactly as the
body path does. The published body is written beside it and stays exactly what
its format says. An item this grid did not take, which means the seeded
`Fixture Object`, has no linkset behind it and still rezzes from its body.

A conformance case names the flavour it needs by naming the *grid*:
`asset-round-trip` declares `Grid::FakeOpensim`, because its fourth leg
reads a taken object's asset back and only OpenSim ever lets a viewer do
that; `object-asset-format` declares both fake grids and is run twice, so
its Second Life leg records `take_step = item-created-nil-asset` — the same
string it records on aditi — and its OpenSim leg records
`item-created-with-asset`.

### Which grid the fake one is

The object asset is one divergence of several, and picking a side per
behaviour is how the fake grid ended up being nobody in particular: before
`imitates::ImitatedGrid` a stock grid announced `platform: OpenSim`, kept
every login field like OpenSim, and withheld a taken object's asset like
Second Life, all at once. A viewer passing against that has not been tested
against anything.

So the grid names the live one it is being, once —
`FakeGridBuilder::imitates(ImitatedGrid::OpenSim)`, default Second Life —
and every divergent behaviour takes its default from that. The
per-behaviour setters still win where they are called; the flavour is what
an unset knob falls back to, not a lock, which is what a test wanting one
deliberate deviation needs. Each resolves once, in `start`.

Nine behaviours follow it today: a taken object's asset, whether the
login response is trimmed to the request's `options` list (Second Life
honours it, OpenSim sends every field regardless), whether
`SimulatorFeatures` carries the `OpenSimExtras` block, which spatial-voice
backend the regions run, the two halves of how inventory works, who
composites an avatar, what the grid charges, and what it says the account
is entitled to. The `SimulatorFeatures` pair is covered under "How a region
introduces itself" below, the inventory pair under "How this grid does
inventory", the bakes under "Who bakes an avatar", and the last two under
"Policy: what the grid charges, permits and refuses".

That second one is small and it immediately earned its keep. Turning it on
by default broke a fake-grid end-to-end test that expected
`map-server-url` in the login response — and the test was right to expect
it while the *client* was wrong: `LoginRequest::new` asked for six options
and consumed a seventh, so against Second Life the grid's map-tile server
URL would simply never have arrived. That is the entire point of a fake
grid that commits to being one real grid.

There is no longer a list of divergences the flavour does *not* decide:
every one this crate has measured is derived from it, the economy price
list having been the last one outstanding. `imitates.rs` keeps that audit,
so a divergence taken one-sidedly in future has somewhere to be written
down rather than rediscovered.

### Who bakes an avatar

`BakePolicy` says whether this grid composites avatars or leaves it to each
viewer, and it is one setting rather than four because the four have to move
together. Second Life central-bakes ("Sunshine"); a stock OpenSim region
runs no bake service and every viewer bakes its own agent and uploads the
result as an ordinary texture asset.

What moves with it:

| what | `ServerSide` | `ClientSide` |
| --- | --- | --- |
| the login `agent_appearance_service` | the per-session route | absent |
| `RegionProtocols` bit 0 | set | clear |
| the appearance's `AppearanceData` block | present, version 1 | no block |
| the `UpdateAvatarAppearance` capability | granted | withheld |

The reason they move together is the failure mode of moving fewer. A viewer
decides per avatar whether that avatar is server-baked, from the
`AppearanceData` block's version
(`setIsUsingServerBakes(appearance_version > 0)`); once it has decided so,
`LLVOAvatar::getImageURL` is the only way it will ever ask for a baked slot,
and with no service URL to build from that function returns an **empty
string**. No request, no failure, no warning — every avatar including the
agent's own stays a cloud and nothing says why. Dropping the service on its
own is therefore worse than leaving it there.

Bit 0 is the half that is easy to forget, because it is about the *agent's
own* appearance rather than about looking at anyone: the reference viewer
reads it as `LLViewerRegion::getCentralBakeVersion()` and never sends
`AgentSetAppearance` in a region that claims to central-bake. A grid that
sets the bit and serves no bake service has an agent that can neither be
baked nor bake itself.

Every client-baked answer is what OpenSim's `LLClientView` actually writes:
`SendRegionHandshake` sends `RegionProtocols = 1 << 63` (bit 0 clear, bit 63
being the unrelated "more than 6 baked textures" extension, which
`ImitatedGrid::region_protocol_bits` contributes separately), and
`SendAppearance` writes a literal zero block count where `AppearanceData`
would go. The fake grid's own bakes are fabricated per session either way
and their bytes live in the grid asset store under the ids the appearance
names, so on the client-baked flavour a viewer reaches them the only road
left: `GetTexture`, by id.

### How this grid does inventory

Two rows of the flavour table rather than one, because they are the same
divergence seen from either end, and because both are *silent* when a viewer
gets them wrong.

**The fetch.** `LegacyUdpInventory` says how the deprecated UDP
`FetchInventoryDescendents` is answered, and it has three settings because
the live grids take two roads and a grid without the path can take either of
two more. An OpenSim-flavoured grid `Served`s it out of the session's own
`SimInventoryTree` — the same tree `FetchInventoryDescendents2` reads —
through `SimSession::send_inventory_descendents`, which packs the reply the
way `LLClientView.SendInventoryFolderDetails` does: at most six folders or
five items per message, folders and items never mixed ("to preserve SL
compatibility", says the comment there), and a nil-id placeholder block
padding whichever half of a message is empty, so an empty folder is one
message of two placeholders rather than no message at all. The client drops
the placeholders on their nil ids, which is a filter nothing else reached.
A Second-Life-flavoured grid `Refused`s it with a `FeatureDisabled`. Aditi
was measured (2026-08-12) silently *dropping* the fetch, and `Ignored`
reproduces that faithfully — but silence is indistinguishable from a lost
packet, so the flavour's default is the observable road rather than the
measured one. This is the one place the table deliberately deviates from a
measurement, and it is written down in `imitates.rs` too.

**The take's announcement.** `InventoryAnnouncement` says how a **taken**
item is handed over: OpenSim's legacy UDP `UpdateCreateInventoryItem`
(`SimSession::send_inventory_item_created`) or Second Life's
`BulkUpdateInventory` over the event queue
(`SimSession::enqueue_bulk_update_inventory`).

Flipping the default to Second Life immediately broke two conformance cases
that waited only for the legacy message and reported a take that had worked
as unacknowledged — which is exactly the failure a viewer would have had,
and exactly why the pair is worth deciding. The fix is one shared helper,
`support::created_item_announcement`, that accepts either shape and says
which arrived; `object-asset-format` then asserts the shape against the
grid's flavour, so a fake grid answering with the wrong one is a failure
rather than something the helper papers over.

It also moved the announcement in *time*, which nothing had predicted. A
take sends the filed item and the world's `KillObject`s in one breath, but
the kills go out over UDP immediately while an event-queue announcement
lands on the client's next long-poll — so on the Second Life side the item
arrives **after** the kills. A consumer that waits for the item and only
then looks for the kills has already discarded them, which is how
`a_taken_linkset_rezzes_back_whole` came to hang rather than fail.

**An upload's announcement, which is not the same answer — and is two
answers.** `UploadAnnouncements` says what follows a **capability upload**,
once for each of the two paths. After an asset saved in place over an
`Update*AgentInventory`, Second Life sends the *legacy* UDP
`UpdateCreateInventoryItem` and OpenSim sends *nothing at all*: the two
grids take the opposite sides from the ones they take on a take. After a
`NewFileAgentInventory` completion **neither grid sends anything**. One
enum could not have said take and upload both, which is why there are two;
one value could not have said creation and save both, which is why the
upload knob is a pair.

Every cell is a measurement, taken 2026-09-08 by the conformance cases that
already did the uploads — `notecard-create-update` records
`save_announcement` (aditi: `update-create-inventory-item`; OpenSim:
`none`) and `asset-upload` records `upload_announcement` (`none` on both).
Taking them needed `support::observe_upload` rather than another
`wait_for`: an announcement is a UDP push and a completion an HTTP
response, so an announcement can arrive *first*, and a `wait_for` looking
for the completion would have discarded it on the way past and recorded the
grid as silent. The reference viewer wants no push anyway:
`LLBufferedAssetUploadInfo::finishUpload` builds the item out of the
response body.

So does this workspace's client, since 2026-09-10: the runtime keeps the
`NewFileAgentInventory` request past the POST and turns the completion into
the item (`sl_proto::uploaded_inventory_item`, which `Event::AssetUploaded`
carries as `created` and the session files before the event goes out). It
had to — a viewer that waits for a push waits forever on every grid there
is, and this one did, so an uploaded item sat on the grid and missing from
the inventory window until something re-fetched its folder. The completion
is also where the item's **permissions** come from: this grid reports what
it granted, as both real grids do, because a grid is free to withhold what
the request asked for.

**The take and the save diverge for opposite reasons**, and reading the
save as "Second Life does something extra" gets it backwards. The push is
the older behaviour and OpenSim is the grid that omits it, which its own
source says twice over: at the in-place save,
`InventoryAccessModule.CapsUpdateInventoryItemAsset` ends on a
commented-out `SendInventoryItemCreateUpdate` and answers with an
`AlertMessage` instead — commented out since 2007-08, when that capability
path was first written, and carried through the 2007-12 rename and the 2010
move into the module still commented — and a `NewFileAgentInventory`
completion reaches inventory through `Scene.AddUploadedInventoryItem`,
which calls the *client-less* `AddInventoryItem` overload sitting right
beside the one that announces. So the take is Second Life having **moved
on** (inventory went behind AIS3 and the announcement went with it), and
the save is OpenSim having **never sent** what a Linden simulator sends —
which is also why Second Life's side of it is the legacy
`UpdateCreateInventoryItem` rather than anything newer.

**Second Life's `NewFileAgentInventory` row was extrapolated from the save,
and it was extrapolated backwards.** Measuring it costs money: that
capability accepts only the chargeable file-upload classes there — it
answers a notecard with `Invalid asset type` — so reaching the completion
needs an upload fee, and sending the right `expected_upload_cost` needed a
price list. Once the account's benefits package made the fee nameable,
`asset-upload` uploaded a 64×64 texture at the account's own price
(L$ 10, charged, twice) and recorded `none` both times.

The reasoning that got it wrong was not silly, which is why it is written
down rather than quietly corrected: the message is the general-purpose
legacy "here is an item you now have", and the grid was measured sending
exactly it for the neighbouring path. What it missed is **what the client
already holds**. A `NewFileAgentInventory` response body carries the whole
new item, so a push would repeat it; an in-place save's response names only
the new asset, and without the push the client's own copy of the item goes
on naming the asset the save replaced. The push survives exactly where it
still carries information.

The legacy UDP transaction save follows neither knob and that is not a gap:
`UpdateInventoryItem`'s `UpdateCreateInventoryItem` is the **reply** to a
UDP request, echoing the transaction and callback ids the client sent, and
OpenSim sends it there (`AssetXferUploader`) precisely where it stays quiet
after a capability upload.

One thing is deliberately not flavour-decided and is not a to-do:
`GridIdentity::platform` stays `OpenSim` either way, because it is what
Firestorm's grid manager reads to decide whether it will add the grid at
all, and a grid Firestorm refuses to add tests nothing.

### How a region introduces itself

`SimulatorFeatures` is where the two grids describe themselves, and they
describe themselves differently in two ways that the flavour now decides.

**`OpenSimExtras`.** OpenSim always sends the block —
`SimulatorFeaturesModule` fills it in unconditionally and `GridService`
injects the grid-wide URLs into it — and Second Life has no such key. It is
the one structural difference that reliably tells the two replies apart, so
a Second-Life-flavoured fake grid omits it
(`FakeGridBuilder::open_sim_extras` overrides).

The part that had to be checked rather than assumed is that **nothing goes
missing with the block**. What rides in it that a viewer actually reads is
the map-tile server, the currency symbol and the currency helper base, and
each has a second route that both grids serve and the reference viewer
reads first when no extras block overrode it: the login response's
`map-server-url` (`LLStartUp::process_login_success_response`), the login
response's `currency`, and `get_grid_info`'s `economy` key
(`LLGridManager::getHelperURI`). `LFSimFeatureHandler` treats the extras
copies as *overrides* of those, not as the only source. So dropping the
block removes a duplicate, not a surface — which is what the assertions in
`http_misc.rs`'s `grid_info_is_served_as_xml_and_xml_rpc` are there to keep
true.

The currency **symbol** is the exception, and it is a real divergence rather
than a hole: stock OpenSim puts no symbol in *either* place. Its login
service defaults `currency` to the empty string and emits the key only
`if (currency != String.Empty)`, and its extras block carries
`currency-base-uri` with no symbol beside it. So a viewer against a stock
OpenSim grid falls back to its own default — `OS$` in Firestorm — while
Second Life sends `L$` in the login response and has no extras block to
copy it into. `ImitatedGrid::currency_symbol` follows that: `Some("L$")`
against `None`.

Modelling it as presence rather than as a second symbol is the point. On
OpenSim the symbol is a *deployment's* choice, not the software's —
`StandaloneCommon.ini` ships `Currency = ""` under "Ask co-operative
viewers to use a different currency name", real grids do set it, and
Firestorm carries a multi-currency subsystem that re-renders its UI when a
region's extras override the symbol mid-session. A fake grid that picked
one symbol for "OpenSim" would be modelling one deployment instead of the
software, and would hide the fallback path a viewer actually takes.

**Voice** (`FakeGridBuilder::voice_backend`).

| | Second Life | OpenSim |
| --- | --- | --- |
| backend | WebRTC (`WebRtcStub`) | none (`VoiceBackend::Silent`) |
| `SimulatorFeatures.VoiceServerType` | `"webrtc"` | absent |
| login `voice-config` | present | absent |
| `RequiredVoiceVersion` push on arrival | sent | absent |
| `ProvisionVoiceAccountRequest` | answered | refused (`BackendUnavailable`) |

The OpenSim column is one decision, not four: with no backend installed
every advertisement falls away on its own, and the provision refuses
itself. That is what a *stock* OpenSim region is — both its voice modules
(`VivoxVoiceModule`, `FreeSwitchVoiceModule`) are optional and off by
default — and modelling the stock region is the same choice
`open_sim_prices` makes for money.

It is also the only honest option here. Both OpenSim modules answer with
the Vivox SIP account shape, and this workspace implements Vivox-shaped
voice nowhere: Second Life removed Vivox for WebRTC, and OpenSim support
for a leaf feature like voice is not a priority. A Vivox flavour would mean
a fixture serving a path nothing in the workspace will ever speak, so there
is no third variant to pick.

Worth knowing anyway, because it is why OpenSim never needed
`VoiceServerType`: a viewer told nothing falls back to Vivox by itself
(`LLVoiceClient::handleSimulatorFeaturesReceived` turns an empty
`VoiceServerType` into `VIVOX_VOICE_SERVER_TYPE`). Against a silent region
it then finds no capability and gives up, which is exactly what a viewer
meets on a stock OpenSim grid today.

### The edit surfaces

Rezzing, derezzing and dropping into a prim were once the *only* writes.
Everything else a viewer can change — the build floater, About Land, the
Region/Estate floater — was decoded by `SimSession` and dropped, so no tier
below a live grid could answer "did my edit reach the grid". The three
families now land, each in a module of its own, and each answered under the
region's own lock:

- **`object_edits.rs`** — the build floater. Two stores travelling in two
  messages, which is the thing to keep straight: the `Object` itself (its
  motion, scale, material, click action and `PrimFlags`) goes out in an
  `ObjectUpdate` and reaches the whole region, while its `ObjectProperties`
  (name, description, category, sale state, permissions, owner, group) go
  out in a message of their own that a simulator sends to whoever holds the
  object *selected*. An `ObjectUpdate` carries none of those fields, so a
  client that renames an object learns the rename took only from
  `SimSession::send_object_properties` — which is why the family needed a
  sender for the full form before any of it was observable.

  Linking is not only a parent id: a child's placement is stated in its
  root's frame, so a link restates it and a delink puts it back. Undo and
  redo are the *simulator's* — the messages name objects and nothing else,
  and address them by full id rather than by the region-local id the rest of
  the family uses — so each edited object carries a short history of whole
  `Object` snapshots (`EditHistory`), which is the only definition of "undo"
  that composes across edits of different kinds.

- **`parcel_edits.rs`** — About Land, and the land a client buys, deeds,
  abandons and reclaims. A parcel has **one** record and a
  `ParcelPropertiesUpdate` carries the whole of it, so an edit is "read the
  parcel, change one field, send it all back" (`ParcelInfo::to_update` is
  that read). A changed parcel is re-sent as a sequence-zero
  `ParcelProperties`, the unsolicited form the arrival burst already uses.
  The access lists are the one parcel record that does not travel in the
  properties reply — they have their own request and reply, and live beside
  the parcels rather than on them.

- **`estate.rs`** — the Region/Estate floater, which is not shaped like the
  other two at all: an estate command is one `EstateOwnerMessage` carrying a
  method name and a list of byte parameters, so the whole floater is a
  switch on a string. `getinfo`, `estatechangeinfo`, `setregioninfo`,
  `setregionterrain`, `texturedetail` / `textureheights` / `texturecommit`,
  `estateaccessdelta`, `estatechangecovenantid` and the map-tile nudge are
  answered; the region's own configuration (`RegionInfo`) and terrain
  composition become writable stores the first time one of them changes
  them, and stay derived from the region's identity until then.

  Every estate command is refused **in silence** for an agent with no estate
  power, which is what OpenSim does and the only thing that makes the gate
  observable. The estate itself is a record and not a rule: a banned agent
  may still log in, because the fake grid enforces nothing.

One deliberate limit: the estate is stored **per region**, because the fake
grid's regions are independent worlds with no store above them; nothing
reads an estate from two regions yet.

### Somebody else changed it

An edit that only its editor is told about is not a simulator's behaviour,
and the difference matters because Second Life has **no arbitration at all**
— no edit lock, no two-phase commit, no consensus. Selection is a
subscription, not a mutex; two residents may hold the same prim or the same
About Land form open indefinitely; latency alone makes conflicting edits the
steady state rather than an error case, and last-write-wins is very probably
the whole of a grid's policy. What makes that survivable is only that the
loser is *told*, so the burden of converging is the viewer's.

Which is why the interesting bug is not "loses the race" — somebody has to —
but **silently reasserting stale state afterwards**. A
`ParcelPropertiesUpdate` carries the *whole* record, so a floater populated
from a read minutes old, with one checkbox flipped, sends every other field
back as it was and reverts whatever somebody else changed in between. The
property worth testing is therefore convergence: after a push, a viewer's
*next* write must carry the pushed values for the fields it did not itself
touch.

`RegionChange` grew the three pushes that make that observable, each going
to a different set of sessions because each surface's subscription is
different:

- **An object's properties** go to the sessions holding it *selected*, and
  to nobody else. `ObjectSelect` / `ObjectDeselect` are typed
  (`ServerEvent::ObjectsSelected` / `ObjectsDeselected`) and each session
  keeps its own selection set, which `run_region_watcher` consults before
  forwarding. A prim's **task inventory** rides on this one: its contents
  serial is a field of the properties record and travels nowhere else, so a
  write into a prim now pushes the record as well as advancing it.
- **A parcel** goes to the avatars standing on it — OpenSim's
  `SendLandUpdateToAvatarsOverMe` — as a sequence-zero `ParcelProperties`,
  the same unsolicited form the arrival burst uses. The fake grid tracks no
  movement, so "standing on" is where the session arrived.
- **The region's own configuration** goes to everyone in the region: there
  is no subscription to belong to, since every avatar is standing in it. So
  do its **ground textures** on a `texturecommit`, whose whole purpose is
  that everybody sees them — the odd one out, because a terrain composition
  travels only in a `RegionHandshake` and a handshake is stamped with the
  *receiving* session's identity, so the region publishes the composition and
  each watcher builds its own message.

The `client_end_to_end` tests stage one two-avatar case per surface, which
a live grid could not: without locking a live interleaving is luck, while
the fake region's lock serialises writes so "A reads, B writes, A writes"
gives the same answer every run. The parcel case runs the whole argument —
a save built from the record read at open reverts the other resident's
rename, and a save built from the pushed record does not.

What a **real** grid does on the same collision is still worth one run to
confirm rather than assume (`test-asset-save-mutation-survey`); the expected
finding is that nothing arbitrates.

Offline conformance: `object-edit` (the whole build surface, including the
transform, the undo stack and the read-back through `ObjectProperties`),
`object-link-delink`, `object-properties`, `parcel-edit` (the About Land
form and the ban list, each refetched and restored), `region-info`,
`estate-info`, `estate-access`, and `asset-round-trip` (every seeded
inventory class fetched, every savable one saved and re-fetched, plus a prim
this case rezzes and drops an item into).

### A parcel's other half

A `ParcelProperties` record has no field for the parcel's **grid-wide** id,
and three request surfaces want one. `SceneFixtures::add_parcel` therefore
takes both halves at once — the record, plus a `ParcelListing` naming the
grid-wide id and the parcel's dwell — and pushing a parcel any other way is
pushing one those three surfaces cannot answer:

- the `RemoteParcelRequest` capability turns a location into that id. The
  runtime registers one `SimParcel` cover per listing when the session
  starts, from the parcel's own bounds, so a cover and the listing behind it
  cannot name different parcels;
- a `ParcelDwellRequest` (region-local id in, `ParcelDwellReply` out) is
  answered with the listing's dwell;
- a `ParcelInfoRequest` for the grid-wide id is answered with a search
  listing *derived* from the two — the name, owner, area and anchor come out
  of the parcel record, the id and dwell out of the listing — so the two
  records cannot drift into describing different land, which is a drift a
  live grid cannot have because both come out of its one land record.

A parcel with no listing is still served: it simply has no grid-wide
identity, so a location inside it resolves to nothing and its dwell and
listing go unanswered, the way a region whose land service is down behaves.

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

`box_prim` makes a plywood cube and nothing else. The default prim texture
(`sl_proto::DEFAULT_PRIM_TEXTURE`) is the one surface a fixture gets for free,
because it is the one a real simulator puts on a prim itself — `box_prim` and
`prim_from_shape`, the object a `RezObject` produces, both wear it. Everything
past that has to be encoded, because
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
    .looping_sound(sound, gain, radius)  // an already-running llLoopSound
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

`fixtures::catalogue()` is the **named catalogue**: nineteen prims, one per
rendering feature, in a west-to-east row 8 m north of the arrival point at
4 m spacing, with every texture, sculpt map, mesh and material they
reference served. `catalogue::entries()` / `entry(name)` give a subject's
id and position, so a check finds "the mesh prim" by name rather than by
a hard-coded local id, and the same fixture backs the automated tiers and
the binary's `--scenario catalogue` — which is what makes a Firestorm
session and a full-stack capture look at the same objects.

### A sounding prim says so twice

The `sound-box` entry is the one prim whose feature is not visible, and it
is worth knowing *why* it carries its sound the way it does. A simulator
states an in-world sound in two unrelated places:

- **On the object.** `llLoopSound` writes `Sound` / `Gain` / `Flags` /
  `Radius` onto the prim and schedules a full update — OpenSim's
  `SoundModule::LoopSound`, whose comment says it plainly: "just sending
  the sound out once doesn't work so well when other avatars come in view
  later on". Stopping it goes the same way (`Sound` nil, `STOP` set). This
  is the only way an avatar arriving *after* the loop started ever hears
  it, and the reference viewer reads the fields back in
  `LLViewerObject::processUpdateMessage`, full and compressed updates
  alike.
- **In a message.** `AttachedSound` (a non-looping `llPlaySound`),
  `AttachedSoundGainChange` (a live `llSetSoundVolume`), `SoundTrigger`
  (a one-shot at a place — `llTriggerSound`, a collision) and
  `PreloadSound` (`llPreloadSound`, and *only* that: no region sends one
  on arrival). Each reaches only whoever is already there.

So `looping_sound(..)` is a fixture for the first, and
`SimSession::send_attached_sound` / `send_attached_sound_gain_change` /
`send_sound_trigger` / `send_preload_sound` are the second. A viewer that
implements only the message half is silent in every region whose sounds
started before it logged in, which is nearly all of them.

The procedural assets it needs come from `sl-test-assets`:
`RgbaImage::checker` / `solid` (as JPEG2000), `sculpt_sphere` (a sculpt
map — geometry stored as a texture), `mesh::unit_cube_mesh_asset` (the
LLSD-binary header plus zlib-compressed LOD blocks `sl-mesh` decodes),
`gltf_material_asset` (the `AT_MATERIAL` LLSD envelope around a glTF 2.0
document) and `sound::marker_tone` (a real Ogg Vorbis tone, which the
`sound-box` loops at concert pitch — a decoder can measure it, and an ear
can tell two of them apart).

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
prim row, baked blue, playing the catalogue's own animation and wearing a
checker box on its skull.

A second one (`catalogue::seated_npc()`) is **sitting**, baked green, one
slot further west on the bench `catalogue::seat()`. An avatar sits by
being parented: its update carries the seat's local id as its `ParentID`
and a position that is the offset from the seat rather than a region
position — the one case where an avatar's own position is not
region-local. `NpcFixture::seated_on` is the whole of it, and both
viewers' scene dumps report the composed region position
(`catalogue::seated_npc_position()`), which is what makes the seated path
comparable rather than merely visible.

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

The library's other half is sound. The twelve **built-in UI sounds** a
viewer plays for its own events — the typing chirp, the money chime, the
teleport whoosh, the snapshot shutter — are library ids too
(`sl_proto::BUILTIN_UI_SOUNDS`), and the reference viewer ships no sound
anywhere in its tree, so a grid that answers none of them leaves every one
of those events silent for the whole session: the fetch fails once, the id
is marked unavailable, and nothing plays again. `default_assets` serves an
Ogg Vorbis tone for each (`sl_test_assets::builtin::library_sounds`), one
whole tone apart over two octaves from A3 up, in the order the shared list
names them — so which built-in just played is something an ear can tell,
and `sl_test_assets::builtin::ui_sound_pitch_hz` is where a test asks
which pitch belongs to which id.

`RegionConfig::environment` is the region's other environmental half: an
`EnvironmentSettings` (day cycle, day length, sky-track altitudes) served
by the `ExtEnvironment` capability. Left `None`, the session's stock
four-hour day answers.

**A sky frame that omits its three scattering profiles is a region with
no environment.** `rayleigh_config`, `mie_config` and `absorption_config`
are marked *required with no default* by the reference's sky validator,
and `Validator::verify` fails a required field it cannot fill — so one
missing key fails the frame, a track with no valid frame empties the day
cycle, and `LLEnvironment::recordEnvironment` refuses the whole thing
with `Invalid day cycle for region`. The viewer then lights the region
with its own built-in sky and reports nothing outside its log. That is
why `SkySettings` carries them as `DensityLayer` lists and why
`legacy_windlight_default` seeds the reference's own defaults: a frame
this crate *constructs* is going on a wire to a viewer that requires
them, whatever a frame it merely *decoded* happened to carry. Found by
pointing Firestorm at this grid on 2026-09-10, and invisible from here
until then — nothing on the grid side had ever complained.

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
   asked) and **registered before the finish names it**, because the
   client contacts the destination the moment `TeleportFinish` arrives
   and an unregistered `/sim/<n>/…` answers 404 to the seed it POSTs;
3. `TeleportFinish` on the source's event queue, and **nothing before
   it** — the full reference record (`TeleportFinishInfo`: agent id,
   region handle, region size, …; Firestorm builds the destination
   region object from the handle, and the client reports the wire handle
   rather than the one it requested, which is what a lure or landmark
   teleport needs). No `EnableSimulator` / `EstablishAgentCommunication`
   precedes it: that is `TransferAgent_V2`'s shape (*"send TP Finish
   directly, without prior ES or EAC. That's what happens in the Linden
   grid"*), and announcing the destination first would make the client
   mistake it for a neighbour it had been holding all along and keep the
   world it should have thrown away;
4. once the destination sees `AgentArrived`, the source is retired:
   `DisableSimulator` to the client, the session closed
   (`ServerEvent::CircuitRetired`, the pumps exit on the per-session
   closed watch), its CAPS paths forgotten, and a `TeleportNotice` on
   `FakeGrid::teleports()`. No arrival within `TELEPORT_ARRIVAL_TIMEOUT`
   fails the teleport with `timeout_tport` and abandons the destination.

A destination the agent already borders is **reused**, not opened again:
it is a child circuit the client is holding, and a second session there
would hand it two simulators for one region handle. On a timeout only a
destination this teleport opened itself is abandoned — a borrowed
neighbour is still a neighbour. Arriving also retires the children of the
region left behind, the way a crossing does, or an agent hopping across
the grid accumulates one open circuit per region it ever bordered.

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

## Neighbours: the region next door

A simulator does not wait for an avatar to reach a border. The moment the
agent is rooted it announces every region within view, the client opens a
**child** circuit to each, and those regions start streaming. That is why
the region across a border is already drawn before you walk into it, and
it is what makes a crossing a *promotion* of an open circuit rather than
a connection made on the spot.

`neighbours.rs` does the same with the one thing a fixture can state:
which regions touch. `RegionConfig::neighbours` is a `NeighbourPolicy` —
`Adjacent` (the default: the eight surrounding grid slots, of however
many the grid actually serves), `None`, or `Named` for a topology the
coordinates do not describe. A per-session announcer task watches for
`AgentArrived` — a login's, a teleport's or a crossing's, so a region
announces its own neighbours however the agent got there — and for each
one that the agent has no session in yet, prepares a `SessionRole::Child`
session and enqueues `EnableSimulator` + `EstablishAgentCommunication` on
the root circuit. It is a task rather than part of the driver's flush
rule because announcing binds a socket per neighbour, which is async
work, and the flush rule runs under the session lock.

A child session never sees a `CompleteAgentMovement`, so its scene has to
go out on `CircuitOpened` or never: `world::push_child_world` sends the
region's objects, its NPCs, its ground and its parcel overlay — the
arrival burst *minus the agent*, because the agent is standing next door.
It ends with a `neighbour:<region name>` marker, which is how a test
waits for "the region next door has finished streaming" without sleeping
(`sl_fake_grid::neighbour_marker_region`, and the viewer harness's
`wait_neighbour`).

Announcing is idempotent by construction: a region the agent already has
a session in is skipped. That matters most right after a crossing, when
the region walked out of is a neighbour of the one walked into and its
circuit is still open — announcing it again would hand the client two
simulators for one region handle. The same lookup is why
`teleport_session` reuses an announced child rather than opening a second
session in the destination.

### Assets are grid-wide

An asset id on a real grid names a blob the whole grid knows: textures,
meshes, animations and settings live behind every region, and a viewer
fetches all of them over its **root** region's `GetTexture` / `GetMesh2` /
`ViewerAsset` — including ids only a neighbour's content references.

`assets.rs` keeps one store per grid, and `FakeGridBuilder::start` folds
every region's fixture assets into it in builder order (a later region
wins a colliding id). A `RegionFixture` still *states* what its own
content needs — that is where a fixture author declares it — but it does
not own a store. The arriving agent's own bakes go in there too, which is
what lets a second avatar's viewer fetch the first's.

It is a plain `std::sync::RwLock`, because the one writer runs inside the
driver's synchronous flush rule; every path takes the session lock before
the asset lock, never the reverse.

This was per region until 2026-09-03, and the symptom was thoroughly
misleading: the marker pillar across a border rendered untextured, so a
checker oracle read "the neighbour region was never streamed" when the
only thing that had not arrived was one JPEG2000 blob.

### An upload goes into that store

`uploads.rs` folds every completed save into the same store, and points the
item that named it at the result. Until it existed the grid answered
`complete` and forgot the bytes, which is not a gap a test can shrug at:
**a save is only observable as a re-fetch.** A viewer that trusts its own
in-memory copy after a save — the bug a round trip exists to catch —
behaves identically against a grid that stored the bytes and one that
dropped them, and so does an editor's Save button.

Three paths reach it, because a viewer has three ways to save:

- **The two-stage CAPS uploader** (`ServerEvent::CapsAssetUploaded`) —
  `NewFileAgentInventory`, `UploadBakedTexture` and every
  `Update{Gesture,Notecard,Script,Settings,Material}{Agent,Task}Inventory`.
  The parked metadata says which: a new file creates an agent item, an
  `Update*` repoints an existing one (in the agent tree, or in an object's
  task inventory, where it also advances the contents serial — a changed
  asset is a changed listing), and a bake names no item at all.
- **The legacy UDP transaction upload** (`ServerEvent::AssetUploaded`) —
  how a *wearable* save reaches a grid, there being no capability for one.
  The bytes are stored under `combine(transaction_id, secure_session_id)`,
  the id the client itself predicted.
- **`UpdateInventoryItem`** (`ServerEvent::UpdateAgentInventoryItems`) —
  the second half of that wearable save, and the only thing correlating it
  with the first. A simulator that reads the upload and ignores this
  message stores an asset no item points at: the item keeps naming what
  the save replaced, and the viewer's next fetch of its own wearable
  answers with the bytes it saved over.

A completion's `new_inventory_item` is the item the upload *replaced* for
every `Update*` family, not a freshly minted id — OpenSim's `ItemUpdater`
answers `uploadComplete.new_inventory_item = m_inventoryItemID`, and it has
to: handing a client an id nothing holds would have it file a second copy
of a notecard it only edited. `NewFileAgentInventory` is the one family
that mints one.

### Every seeded item names bytes

`sl_test_assets::inventory` is one real asset body per inventory class the
workspace can write one for — texture, sound, landmark, clothing, body
part, notecard, script, animation, gesture, mesh, settings, material — with
the id an item declares and the bytes that id resolves to, plus a *second*
body of the same class for the save half (a round trip that re-fetches the
id it was handed proves nothing if the bytes never changed). The stock
scenario seeds one item per entry, filed in the system folder its class
belongs in.

This replaced a "Party Hat" and a "Library Texture" whose asset ids were
their item ids plus a constant, pointing at nothing, both declaring class
`texture` whatever their names said. An id a viewer is given is an id it
will eventually fetch, so those items looked fine in an inventory window
and failed at every attempt to *use* them.

`AssetType::Object` was the last class with no codec at all — an object
asset is `LLViewerObject`'s nested-block text, unrelated to the
`ObjectUpdate` wire form a fixture builds — and `sl-object-asset` closed
that, so a `Fixture Object` is seeded like every other class.
`Gesture` has a body but no *decoder*, so it is the one entry whose round
trip is byte-level only. `inventory::unsupported_classes()` carries the
reason for every class with no fixture, and a crate test fails if a class
has both a body and a recorded reason, or neither.

## Walking over a border

`crossing.rs` is the teleport's quiet sibling, and it differs in three
ways that all matter to a viewer:

- **No teleport screen.** One `CrossedRegion` event and the client
  promotes a circuit it already holds. The scene is *kept* and re-based
  onto the new origin rather than torn down and rebuilt, which is what
  the client reports as `RegionChanged { world_reset: false }`.
- **The destination is already open**, as the neighbour announcement left
  it. Only a crossing into a region the announcement missed opens one on
  the spot.
- **The source is not retired.** It becomes a child agent
  (`SimSession::make_child_agent`, OpenSim's `MakeChildAgent`) and keeps
  streaming, because the region you just walked out of is still in front
  of you. Only the children that have dropped out of view are retired
  (`retire_distant_children`).

The departing avatar's object is deliberately **not** killed on the
source circuit. The reference simulator kills it only for *other* viewers
that cannot see the region the avatar walked into, never for the crossing
agent's own client — and killing it here would be worse than merely
unfaithful: this viewer keys avatars by agent, not by circuit, so a kill
arriving on the old circuit after the new one has streamed the body
despawns the avatar outright.

The event body is the full reference record
(`sl_proto::CrossedRegionInfo`): `AgentData` (agent and session id),
`Info` (position and "look at") and `RegionData` (handle, seed, address,
region size). All three blocks, because the reference viewer's
event-queue translation feeds the body straight into the legacy
`CrossedRegion` message and `process_crossed_region` *rejects* one whose
`AgentData` does not name its own agent — a body carrying `RegionData`
alone reaches this workspace's client, which reads only that block, and
is thrown away by Firestorm. The `Info.LookAt` field is named for the
wire and not for its contents: OpenSim puts the crossing agent's
horizontal **velocity** there, so the client keeps its momentum.

Because the fake grid claims no movement authority, a crossing is asked
for rather than noticed: `FakeGrid::cross_agent(&agent, "Region",
position, velocity)`, which refuses a destination that does not border
the agent's region (`Error::NotAdjacent`) and publishes a
`CrossingNotice` on `FakeGrid::crossings()`.

A crossing has the teleport's arrival timeout too: no `AgentArrived`
within `CROSSING_ARRIVAL_TIMEOUT` and the agent stays where it was
(`Error::CrossingTimedOut`). The cleanup rule is the teleport's, one
destination shape narrower — a crossing destination is nearly always a
neighbour whose child circuit the *announcement* opened, so the failure
is not entitled to take it down. `FakeGridBuilder::handover_timeout`
shortens both budgets, which is what makes either failure a test rather
than a thirty-second wait.

### Sitting, and riding across a border

A sit is a conversation: the client's `AgentRequestSit`, the region's
`AvatarSitResponse`, the client's completing `AgentSit`. The fake grid
answers one for any object it has (`world::answer_world_request`) and then
does the visible half — re-sending the agent's own avatar object with the
seat's **region-local id as its `ParentID`** and a position that is the
offset from the seat rather than a place in the region. That is the one
case where an avatar's position is not region-local, and it is how every
client learns someone is aboard something.

The offset is a fixed `SIT_TARGET_OFFSET`, not the point the client
clicked: a real vehicle sets an `llSitTarget`, so riders snap to the seat.

A **ridden** crossing is the interesting case, because the vehicle is
handed over too and everything about it is renumbered. An object keeps its
grid-wide `ObjectKey` across a border and takes the destination's own
region-local id, so a viewer that keyed a rider's seat by local id alone
would lose the seat at the line. `fixtures::border::BorderSide` builds the
pair: `Leaving` stands the vehicle against its region's east edge,
`Arriving` against the next region's west edge, a few metres apart across
one border, same full id and different local ids. `FakeAgent::with_world`
(mutate a session's fixtures *and* send, under one lock) and
`FakeAgent::seat_on` are what a test drives the handover with.

`border_pair_side(side)` is that half made whole, and is what the
`border` scene dresses each of a pair's two regions with: this side's
painted ground, this side's vehicle with the rider aboard, and the
marker pillar. It is what a two-region cross-check run photographs.

The one thing it has to say out loud is **which ids are shared and
which are not**, because a pair grid streams both regions at once and a
viewer keys an object by its grid-wide id:

- the **vehicle** keeps one id either side, deliberately — a vehicle
  really is one object being handed over — so a pair grid holds one
  vehicle, at whichever side streamed it last, and the crossing is what
  makes that the right answer rather than a bug;
- the **rider** likewise, so a pair shows one rider and not two, and a
  crossing ends with two avatars in the scene (yours and it) rather
  than three;
- the **pillars** get an id each (`BorderSide::marker_object`), because
  a pillar is scenery that belongs to one region. Sharing one id there
  did not produce a pillar per side — it produced a single pillar being
  moved from one region to the other, which is what Firestorm was
  observed doing before the ids were split.

## Scripted timelines

Every surface above answers something the client asked for. A
`Scenario::timeline` is the other half: what happens to a session because
**time passed**, which is what a test of anything *moving* needs — a prim
that moves, an avatar that starts an animation, a region whose sky
changes, an agent walked over a border.

```rust,ignore
let timeline = Timeline::new()
    .then(At::AfterArrival(Duration::from_secs(2)),
          Action::MoveObject { local_id, to })
    .after(Duration::ZERO, Action::Marker("moved".to_owned()))
    .then(At::OnMarkerAck, Action::KillObject(local_id));
```

A step is an `At` and an `Action`. The `At` is a duration from the
session's arrival (`AfterArrival`), a duration from the previous step
(`AfterPrevious`), a `ServerEvent` the session drains (`OnEvent`, tested
against every event since the script started, so a step waiting for
something that already happened runs at once), or the client's own
acknowledgement of the last marker (`OnMarkerAck`).

`OnMarkerAck` is the one wait with a happens-before behind it rather than
a guessed number of milliseconds. A client acknowledges a packet it has
decoded and handled, so once the marker's ack is in, everything sent
before it has already reached the viewer's own event stream — which is
exactly what a test wants before it takes a second screenshot. It is also
the one wait that does **not** stop the script when it times out: an
ordering nicety that could strand a run would be worse than the
reordering it prevents, which is the call `teleport.rs` already makes
about its own `TeleportStart`. An `OnEvent` that times out *does* stop the
script, because there the wait is the whole point of the step and the rest
of the script was not written for a world where it never happened.

The `Action` is anything a simulator does unprompted: `RezObject`,
`MoveObject`, `UpdateObject`, `KillObject`, `Attach` / `Detach`,
`AnimateAvatar`, `SetAppearance`, `Chat`, `Im`, `SetEnvironment`,
`PushExperienceEnvironment`, `ReportExperienceEvent`, `ConfigureRegion`,
`ChangeParcel`, `Teleport`, `CrossRegion`, `SimStats`, `SimulatorTime`,
`Marker`, and `Custom` for a hook.

`SetEnvironment` and `ConfigureRegion` go together, and the pairing is a
protocol fact rather than an inconvenience. Nothing carries new environment
settings *to* a viewer that is already standing in the region: `SetEnvironment`
changes what the `ExtEnvironment` capability answers, and the viewer's reason to
ask again is a `RegionInfo` — which is what `ConfigureRegion` sends. The
reference viewer re-reads on every one of those without comparing a field
(`LLViewerRegion::processRegionInfo` runs `LLRegionInfoModel`'s update signal,
which `LLEnvironment` has hooked to `requestRegion()`), and it could not do
otherwise: a `RegionInfo` carries no environment fields at all. So an estate
that changes only the sky saves the Region tab without moving a limit, and a
script says the same thing with an empty `ConfigureRegion` edit.

The one environment change that really is *pushed* is a different action —
`PushExperienceEnvironment`, an experience's `llSetEnvironment`, which travels
as a `GenericMessage` (`PushExpEnvironment`) and needs no `ConfigureRegion`
beside it. It layers **over** the region's settings rather than replacing them,
so its `Clear` case puts the region's own sky back with no refetch — the
settings underneath were never overwritten, only covered. Its three cases are
the reference's own: `Clear` releases one experience (or, with a nil experience
id, every one of them), `Full` names a settings **asset** by id — which the
viewer fetches over `ViewerAsset`, so the scenario has to have put those bytes
in the grid's asset store — and `Partial` carries a sky and/or water fragment
whose keys are overlaid on whatever is in force. The experience id rides in the
message's invoice, not in the parameter list.

`ReportExperienceEvent` is the other half of what a region says about an
experience, and the only half that is *about* the experience rather than about
the sky. An experience the agent has joined runs its scripts without prompting
for each permission — that is what joining one buys — so no `ScriptQuestion`
ever appears for what it does, and nothing else in the session mentions it. The
region instead reports it afterwards, as an `ExperienceEvent` generic message
carrying the permission index, the owner, whether the acting object was an
attachment, and the object and parcel names. It is the only message on any path
that says an experience **attached** something to the agent, which is why the
viewer's experience log exists to keep it.

A real region sends one of these beside a `PushExperienceEnvironment`; this grid
does not, deliberately, so a test can drive either half alone — the sky change
without the paper trail, or the paper trail without touching the sky.

The world-changing ones go through the region's shared store and publish to its
change stream, so a second avatar standing there is told as well — a scripted
rez is a rez, not a picture painted on one circuit.

Every wait is a `tokio` sleep and every stamp comes from the grid's
injected clock, so a test that pauses tokio's timer pauses the script with
it: a scripted minute costs no wall-clock time.

### The script belongs to the avatar

A script that says "teleport, then move the prim you find there" has to
outlive the session it started in — a teleport destination is always a
*second* `SimSession`, and a crossing promotes a circuit the client
already held. So once the client has actually arrived, the steps that have
not run yet are handed to the destination's session and the one left
behind keeps only the prefix it ran. That happens for a
**client-initiated** teleport too, which is the point: the script belongs
to the avatar, not to the patch of land.

A destination whose own region declared a timeline loses it to the
incoming one; a script that has already finished hands over nothing, which
is what leaves the destination's own script alone. The runner parks on a
notification rather than exiting when it runs out of steps, so it does not
matter whether the script arrives before or after that session's arrival,
and it carries a generation alongside its cursor so a runner waiting on
step *n* of one script can never execute step *n* of the script that
replaced it.

A runner stops when its session closes, when the grid shuts down, or when
the agent stops being the root agent there — which is what a crossing
makes of the region left behind.

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

## The offline conformance tier

`sl-conformance` is the third consumer, and the one whose subject is the *wire*
rather than the picture. `sl-conformance::fake` starts a grid with two regions —
the catalogue and the border scene announced as its neighbour — registers three
accounts and synthesises the credentials that reach them, so the conformance
runner's ordinary login path (XML-RPC round trip included) reaches an offline
grid. The cases in `fake::OFFLINE_CASES` then run as plain `cargo test` tests
instead of waiting for someone to log a live grid in.

The list grew again with the request surfaces the simulator half was missing:
`economy-data`, `parcel-info-dwell` and `asset-fetch-http` now run offline
because `SimSession` learned to surface an `EconomyDataRequest`, a
`ParcelDwellRequest` and an `AgentWearablesRequest`, and `agent-alert` and
`server-error` joined them once the grid grew the *policy* behind their
provocations (see above) — both of those used to pass by recording `partial`
after burning their whole reply window, which is not the same as passing.

Two of them can only exist here. `region-crossing` needs the harness to speak
*as* the simulator — a crossing is a decision a region makes, and this grid
simulates no movement to make it with — so `TestContext::fake()` hands the case
`FakeGrid::cross_agent`; and `neighbour-child-circuits` needs two adjacent
regions an avatar may walk between, which neither live grid reliably offers.
See the *conformance testing* chapter.

### The logins that are refused

Every conformance case starts from a login that *succeeded* — a `TestContext`
is assembled out of live sessions — so no case can be the one that asserts a
login was declined. `sl-conformance/tests/login_refusals.rs` is the other half,
and it uses no registry at all: one grid built per case with exactly the gate
under test set, driving `sl_client_tokio::Client::connect` straight at it.

`FakeGridBuilder::gates` and `AccountConfig::mfa` between them cover every
reason a real grid declines a correctly-addressed login, and each has to reach
the client as something it can act on — a `tos` a viewer can put a dialog in
front of, an MFA challenge it can answer, a `presence` it may retry:

- a wrong password and an unknown account, which must be **indistinguishable**
  or the endpoint tells a caller which names exist;
- `tos` and `critical`, refused with the text to display and cleared by the
  same login re-sent with `agree_to_tos` / `read_critical`. Note that
  `LoginRequest::new` leaves both flags *set*, which is right for a driver
  logging into a grid it has already agreed with — a viewer that has to show
  the terms sends its first attempt without them;
- `presence`, classified as retryable from the message rather than the reason
  code;
- an MFA challenge, answered by the one-time code or by echoing the
  `mfa_hash` the challenge handed out ("remember this device") — and *not*
  raised for a wrong password, which would tell an attacker the password was
  right;
- a redirect, followed to the grid that answers it, and abandoned at the hop
  bound when it loops.

`FakeGridBuilder::stale_presence` is the ghost rather than the gate: it refuses
**one** login as already-logged-in and the refusal itself clears it, which is
what OpenSim's login service does on its way to reporting one. That is the
whole reason a driver may retry such a rejection at all, and the conformance
runner's retry branch — the only production code in this workspace that reacts
to an `AlreadyLoggedIn` — had no way to be exercised until a grid could both
refuse and then relent.

## Policy: what the grid charges, permits and refuses

Not every answer is content. Three of them are policy, and they live apart
from the fixtures because they are decisions about the *grid* rather than
statements about a region.

`EconomyConfig` carries the whole money policy, both halves of it: the L$
rate its web helper quotes over `currency.php` and the price list its
simulator answers an `EconomyDataRequest` with (`EconomyConfig::prices`).
One config, because a grid whose helper quoted one rate while its simulator
quoted another is a grid no viewer can reconcile.

The price list follows the flavour (`ImitatedGrid::prices`,
`EconomyConfig::for_grid`), and both columns are the real grid's rather
than this crate's invention:

| field | Second Life | stock OpenSim |
| --- | --- | --- |
| `object_capacity` | 20 000 LI | 15 000 LI |
| `object_count` | 0 | 0 |
| `price_energy_unit` | 100 | 0 |
| `price_object_claim` | 10 | 0 |
| `price_public_object_decay` | 4 | 4 |
| `price_public_object_delete` | 4 | 0 |
| `price_parcel_claim` | 1 | 0 |
| `price_parcel_claim_factor` | 1.0 | 1.0 |
| `price_upload` | 10 | 0 |
| `price_rent_light` | 5 | 0 |
| `teleport_min_price` | 2 | 0 |
| `teleport_price_exponent` | 2.0 | 2.0 |
| `energy_efficiency` | 1.0 | 1.0 |
| `price_object_rent` | 1.0 | 0.0 |
| `price_object_scale_factor` | 10.0 | 10.0 |
| `price_parcel_rent` | 1 | 0 |
| `price_group_create` | 100 | none stated (`-1`) |

The Second Life column is one aditi run of the `economy-data` conformance
case (2026-09-08), which records all seventeen fields for exactly this
purpose. The OpenSim column is read off `SampleMoneyModule` — its field
initialisers and its `[Economy]` config defaults — rather than measured,
because the local OpenSim grid deliberately overrides `PriceUpload` and
`PriceGroupCreate` in `bin/OpenSim.ini` so a live round trip is observable;
its `economy-data` record therefore confirms fifteen of the seventeen and
differs from stock in exactly those two.

**The OpenSim column is not a table of zeroes**, and the five fields where
the two grids still agree are the residue of a copy. OpenSim's money module
once shipped a near-verbatim copy of a Linden simulator's price list: its
pre-2018 defaults were `100`, `10`, `4`, `4`, `1`, `1.0`, `5`, `2`, `2.0`,
`1`, `1.0`, `10`, `1` — thirteen of fifteen identical to what aditi
answered in 2026, the exceptions being the upload charge (`0` against
Second Life's L$ 10) and the group price. A 2018 commit then zeroed most of
them "to no cost values, since that is our default", and what survived is
exactly `PricePublicObjectDecay`, `PriceParcelClaimFactor`,
`TeleportPriceExponent`, `EnergyEfficiency` and `PriceObjectScaleFactor`.
The agreement is history, not policy.

**One field arrives negative**, and it cost a decoder fix to model: a stock
OpenSim region sends `-1` for `PriceGroupCreate`, which this workspace used
to reject as an out-of-range L$ amount — dropping the whole reply, so a
viewer against an unconfigured OpenSim grid learned no price at all rather
than sixteen prices and one blank. `EconomyData::price_group_create` is an
`Option<LindenAmount>` now, `None` for any negative. The other price fields
keep the strict decode: no simulator has been measured sending a negative
for one, so a negative there is still a malformed message worth dropping.

**Read that `None` as "unknown", not as "free"** — the `-1` is not a
considered sentinel and semantically it is nonsense as a price, since `0`
was available and says exactly that. Three things explain it and none of
them is intent. It is the value the reference viewer's own `LLBaseEconomy`
initialises *every* price to before a reply arrives, meaning "not received
yet", and it reached OpenSim's config default from there. It sits in a
display-only field: OpenSim charges group creation from
`IMoneyModule.GroupCreationCharge`, hard-coded `0` in `SampleMoneyModule`
and guarded with `if (charge > 0)`, and never consults this number — while
the modern reference viewer prices group creation from the account's
benefits package (`create_group_cost`) rather than from this reply. And the
2018 commit that set the field initialiser to `-1` is the one quoted above
as setting "no cost values": it moved all fourteen sibling prices *to* `0`
in the same breath. So the author meant free, and this is the single field
that spells free differently from its neighbours.

Because the two lists differ in twelve of seventeen fields, the
`economy-data` case asserts the whole table field-for-field on both fake
flavours. What that catches is a grid quoting the wrong grid — the wiring,
not the codec. It is deliberately *not* a replacement for the
encoder-slot check the synthetic all-distinct table used to provide: both
sides of the comparison come from the same constant, and neither real list
is all-distinct. That check stayed where it belongs, in `sl-proto`'s own
`send_economy_data` round trip.

### What the account is entitled to

The price list is not where a modern viewer reads upload costs on Second
Life. `LLAgentBenefits` reads them from the login response's **benefits
package**, and Firestorm's `OpenSim legacy economy` patches fall back to
`EconomyData`'s `price_upload` *only when the grid is not Second Life* — so
the legacy field is the OpenSim path and the benefits package is the Second
Life one, the opposite way round from how it reads.

`ImitatedGrid::describes_account_entitlements` decides whether the grid
sends any of it. Second Life sends `account_type` (the package the account
is on), `account_level_benefits` (that package's numbers) and
`premium_packages` (every package's numbers, so a viewer can render "Premium
would give you N"). A stock OpenSim grid sends none of the three — its login
service has no notion of a subscription — which is why Firestorm gates its
whole benefits init behind `isInSecondLife()`: the reference parse *fails*
on a missing field and insists on seeing both `Base` and `Premium`, so a
grid sending half of this would make a viewer complain at every login.

The table `sl-fake-grid` serves is measured, one aditi login on 2026-09-08
(`sl-conformance`'s `login-handshake` records all of it):

| package | texture | 2K texture | sound/anim | group | groups | animesh |
| --- | --- | --- | --- | --- | --- | --- |
| `Base` | 10 | **50** | 10 | 100 | 50 | 1 |
| `Plus` | 10 | 50 | 10 | 100 | 55 | 1 |
| `Premium` | 10 | **40** | 10 | 100 | 80 | 2 |
| `Premium_Plus` | **0** | **0** | **0** | **10** | 150 | 3 |
| `Premium_Plus_No_Stipend` | 0 | 0 | 0 | 10 | 150 | 3 |

**Texture uploads are tiered**, which is the thing `EconomyData` cannot
express: above `MIN_2K_TEXTURE_AREA` (1024×1024) a texture costs L$ 50 on
`Base` against L$ 10 below it, while the legacy reply quotes a flat 10. A
client sending the legacy figure as its `expected_upload_cost` for a large
texture is refused and told nothing useful.

Aditi sends **five** packages, not the two the viewer demands, so the fake
grid sends five: the shape a viewer meets on the real grid includes tiers it
has no special knowledge of. `picks_limit` (20) and `attachment_limit` (38)
are identical on all five, so a viewer gating either on the subscription
would be gating on nothing.

### The maturity trio

Three login fields, and the divergence is again mostly one of presence.

| field | Second Life | stock OpenSim |
| --- | --- | --- |
| `agent_access_max` | per account | hard-coded `A` |
| `agent_region_access` | per account | **absent** |
| `agent_access` | `M` (see below) | hard-coded `M` |

`agent_access_max` is the entitlement, and it is the one a client's
`canSetMaturity` rule reads. `AccountConfig::maturity_ceiling` sets it, and
it exists because until it did **every fake account was entitled to
everything** — so that rule had never once been exercised against an account
that could fail it.

`agent_region_access` is, despite its name, not a property of a region: the
reference viewer reads it as the account's *preference* and seeds
`PreferredMaturity` from it. No OpenSim grid sends it — the field appears
nowhere in OpenSim's sources — and Firestorm handles the absence
deliberately, defaulting the preference to the ceiling (FIRE-8854).

`agent_access` is the least understood, and the fake grid copies the
measurement rather than deriving it. Aditi answered `M` on three runs whose
ceiling *and* preference were both `A`; OpenSim hard-codes `M` for everyone.
Two readings fit: a **clearance** (what the account is cleared for as
against what its type permits — an unverified account is cleared to Moderate
while entitled to Adult, and the same axis carried the pre-2010 Teen Grid
restriction), or the **start region's own rating**. That avatar's start
region is itself Mature, so the run cannot separate them; what it does rule
out is a vestigial constant, since the value coincides with something rather
than sitting where it was left. A login at a differently-rated region, or an
age-verified avatar, settles it in one run.

`AgentPolicy` is the per-session half — what *this* agent may do:

- **Estate powers** (`AccountConfig::estate_manager`, `false` by default).
  OpenSim returns without a word from an estate command an agent has no
  power for, and so does the fake grid, so the check is a check: the
  conformance grid registers only its primary account as a manager, which is
  the same thing a live OpenSim run has to arrange by hand. With the power,
  the one estate command the grid answers — the viewer's `refreshmapvisibility`
  nudge — replies with OpenSim's own "Terrain map generated" `AlertMessage`.
- **The deprecated UDP inventory fetch**
  (`FakeGridBuilder::legacy_udp_inventory`, which otherwise follows the
  flavour — see "How this grid does inventory" above for the three
  settings and why the Second Life default is the refusal rather than the
  measured silence).

Set-Home is policy of a third kind: *every* outcome is answered, which is
what makes it the one deterministic way to provoke an `AgentAlertMessage`.
Which outcome follows OpenSim's rule — the land's owner may set home on it
and nobody else may — so on the catalogue region, whose land belongs to a
fixture creator, an ordinary avatar gets the refusal.

The agent's **outfit** is the fourth: the simulator holds it
(`SimSession::set_agent_wearables`, seeded by `default_setup` from the same
table the Current Outfit Folder links are built from), and an
`AgentWearablesRequest` is answered from that record rather than from the
folder — as a simulator does, because the outfit is appearance state and the
COF is the inventory record shadowing it. The four library body parts the
stock account wears are also **served**:
`sl_test_assets::builtin::library_wearables` writes an `LLWearable`
stand-in per id. A viewer that ships them answers them
locally and never asks, but a grid that dresses an avatar in an asset it will
not serve is lying about what it has — and anything without those static
assets (a conformance case pulling a worn asset over `ViewerAsset`) fetches
them like any other asset.

## Voice signalling

A stock Second-Life-flavoured grid speaks **WebRTC voice**. The two halves
come from different places, deliberately: `default_setup` files the stock
parcel's estate-wide channel (its `channel_uri` is the region id, the form
Second Life sends) with the agent standing on it — a parcel's channel is
scene fixture, it says *where* voice happens — while the **backend** that
serves it is the grid's, installed by the runtime from
`ImitatedGrid::voice_backend` after the scenario's setup has had first
refusal. A scenario that enables one itself keeps it.

Every backend advertisement then derives from the backend that ended up
installed: the login response's `voice-config`,
`SimulatorFeatures.VoiceServerType`, and a `RequiredVoiceVersion` push over
the event queue when the avatar arrives. A `VoiceBackend::Silent` region —
the OpenSim flavour — advertises none of them and refuses a provision
request with `BackendUnavailable`.

A client's `RequestVoiceAccount` (WebRTC offer) is
answered with a JSEP answer, its `SendVoiceSignaling` trickle is recorded
on the connection, `RequestParcelVoiceInfo` returns the region-id
channel, and a logout closes the session; the grid side sees
`VoiceProvisionRequested` / `VoiceSignalingReceived` /
`ParcelVoiceInfoRequested`. No media plane: nothing listens on the
advertised loopback candidate, so a real viewer will negotiate and then
sit in "connecting" — the signalling, not the audio, is what this
exercises. Chat-session channels can be gated with
`set_channel_credentials(channel, credentials)`.

## The experience catalogue

Experiences are a Second Life feature — stock OpenSim ships no experience
module — so a viewer's Experiences floater has, historically, had exactly one
grid it could be pointed at, and that grid costs an account, a login and a
network. `default_setup` seeds the offline other one (`sl-fake-grid`'s
`experiences` module): a set of records the `GetExperienceInfo`,
`FindExperienceByName` and `UpdateExperience` capabilities answer from, owned
by a fixture resident whose display name is registered beside them so the
viewer's Owner column resolves to a name.

It is deliberately bigger than the handful of hand-written records it needs to
cover the corners of the record (grid-wide, privileged, group-owned, and one
**private** one, which is in the catalogue precisely to be absent from search
results). `FindExperienceByName` is paged, and its reply states whether there
is a page on either side of the one it carries — a catalogue small enough to
fit one page can never make the grid say *yes* to that, so it can never
exercise a viewer's paging arrows. The filler records therefore number one more
than a whole page, so a search for them spills onto a short second page and the
boundary is visible from both sides.

What the catalogue does **not** seed is the agent's own five relationships —
allowed, blocked, owned, admin, contributor. A scenario's `setup` hook runs
before the circuit is open, so the session does not know the agent id yet, and
a fixture claiming the agent owns an experience would have to name somebody
else as that experience's owner. Those five tabs therefore come back empty for
now; see the `server-fake-grid-agent-experiences` roadmap item.

## What is deliberately still small

The stock `Scenario` is intentionally small (an inventory skeleton, a library
of the twelve textures above, one parcel, one box, a chat greeting, WebRTC
voice signalling, the experience catalogue above). A real viewer
will ask for much more — terrain, appearance, textures — and renders a login
into a nearly empty world; growing the default scenario against what a viewer
actually requests is expected iteration, not a bug. Firestorm's seed-request
retries (up to 30×) are harmless: the grant is minted once, so every retry
gets a byte-identical reply.
