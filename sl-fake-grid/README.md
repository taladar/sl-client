# sl-fake-grid

An in-process loopback fake Second Life / OpenSim grid built on the
workspace's sans-I/O server machinery: `sl-wire`'s login server,
`sl-proto`'s `SimSession` (the simulator-side protocol machine) and
`SimCaps` (the capability dispatch). This crate adds only the I/O glue —
an HTTP endpoint serving login and CAPS (including the `EventQueueGet`
long-poll), one loopback UDP socket per logged-in session, and scriptable
content fixtures — including the legacy UDP asset paths (named `Xfer`
files, task inventories, `TransferRequest` sources, the estate terrain RAW
heightmap) and the world burst a simulator pushes on region entry (the
agent's own avatar, the parcel overlay, the agent's parcel, the region's
ground as `LayerData` terrain/wind/cloud patches, the region's objects),
replayed on request. No world authority, no persistence: content is
whatever the scenario scripts.

Two consumers by design:

- **Integration tests**: `FakeGridBuilder` starts a grid on ephemeral
  ports inside the test process, so tests run in parallel; the returned
  handles (`FakeGrid`, `FakeAgent`) let the test drive the grid side of
  the conversation (send chat, push object updates, enqueue CAPS events)
  and assert on the `ServerEvent` stream. Both the tokio client and the
  Bevy plugin (`sl-client-bevy/tests/fake_grid_login_smoke.rs`) log into
  it in their end-to-end tests.
- **Manual viewer testing**: the `sl-fake-grid` binary serves a grid an
  unmodified viewer (this workspace's, or Firestorm's grid manager) can
  log into at `http://127.0.0.1:<port>/` — the highest-fidelity offline
  test target this workspace has short of a real grid.

Next to login and CAPS the port also serves the non-CAPS surfaces a grid
manager and the world map expect: `GET /get_grid_info` (and the XML-RPC
`get_grid_info` method on `/`), world-map tiles at
`/map-<zoom>-<x>-<y>-objects.jpg` (the login response's `map-server-url`
points back at the grid), and the economy helper scripts
`/currency.php` + `/landtool.php` for the buy-L$ / buy-land flows.

The stock scenario also speaks WebRTC **voice signalling** (offer →
answer, ICE trickle, parcel channel, logout — no media plane) and
advertises it the way a Second Life region does (`voice-config`,
`SimulatorFeatures.VoiceServerType`, `RequiredVoiceVersion`).

## Neighbours, teleports and crossings

A grid with more than one region behaves like a real one about its
**neighbours**: the moment an agent is rooted, every region touching its
own (`RegionConfig::neighbours`, `NeighbourPolicy::Adjacent` by default)
is announced over the event queue, the client opens a child circuit, and
that circuit is handed the neighbour's objects, avatars and ground. This
is why the region across a border is drawn before you reach it.

`FakeGrid::teleport_agent` and `FakeGrid::cross_agent` are the two ways
an agent moves between regions, and they are deliberately different:
a teleport puts up the teleport screen, hands the client a
`TeleportFinish` and retires the source circuit; a crossing sends one
`CrossedRegion`, promotes the child circuit the client already holds
without any screen at all, and leaves the source open as a child. The
grid claims no movement authority, so a crossing is asked for rather than
noticed.

## Content fixtures

Assets are **grid-wide**. A `RegionFixture` states the ids its own content
references, and the builder folds every region's into one store when the
grid starts — because an asset id names a blob the whole grid knows, and a
viewer fetches every one of them over its *root* region's capability,
including the textures of the neighbour it can see across a border.

`fixtures::PrimFixture` builds the `Object` records a region pushes:
every builder method sets a typed value and packs it into the raw wire
blob beside it (`texture_entry`, `extra_params`, `particle_system`,
`texture_anim`), which is the only form an `ObjectUpdate` carries. A
`fixtures::RegionFixture` is one region's whole content — objects,
assets, legacy materials, per-face media, environment, ground — and
`into_region` wires each piece to the surface that serves it.

`fixtures::catalogue()` is the **named catalogue**: one prim per
rendering feature (textured, sphere-shaped, per-face styled, mesh,
sculpt, PBR, legacy material, projecting light, flexi, particles,
animated texture, hover text, media, reflection probe, linkset) in a
west-to-east row north of the arrival point, with every asset it
references served. The automated tiers and the binary's `--scenario
catalogue` load the same fixture, so "the mesh prim" is the same object
with the same id in a unit test, a full-stack capture and a Firestorm
session.

`fixtures::border()` is the **border** scene: one checkered marker pillar
floating just inside the region's west edge. It exists for the questions
that need two regions — is the region across the border drawn at all, and
does it stay put when the avatar walks into it — because both are only
decidable in pixels if the subject's position is stated relative to a
border rather than to the middle of a region.

`fixtures::scenarios` names the scenes — `stock`, `catalogue` and
`border` today — so a harness selects one by name and the next one is a
registry entry rather than a change to the harness. Each scene names its
landmarks (a name and a region position per thing worth aiming a camera
at).
`scripts/fake-grid.sh` starts the binary on a fixed port with a named
scenario and prints, once the grid answers `get_grid_info`, the login URI
as an IPv4 literal plus the `--grid` argument Firestorm wants.

See the book chapter "The fake grid" for architecture and usage.
