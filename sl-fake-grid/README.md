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

## Content fixtures

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
references served. The automated tiers and the binary's `--catalogue`
flag load the same fixture, so "the mesh prim" is the same object with
the same id in a unit test, a full-stack capture and a Firestorm
session.

See the book chapter "The fake grid" for architecture and usage.
