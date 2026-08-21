# sl-fake-grid

An in-process loopback fake Second Life / OpenSim grid built on the
workspace's sans-I/O server machinery: `sl-wire`'s login server,
`sl-proto`'s `SimSession` (the simulator-side protocol machine) and
`SimCaps` (the capability dispatch). This crate adds only the I/O glue —
an HTTP endpoint serving login and CAPS (including the `EventQueueGet`
long-poll), one loopback UDP socket per logged-in session, and scriptable
content fixtures — including the legacy UDP asset paths (named `Xfer`
files, task inventories, `TransferRequest` sources, the estate terrain RAW
heightmap). No world authority, no persistence: content is whatever the
scenario scripts.

Two consumers by design:

- **Integration tests**: `FakeGridBuilder` starts a grid on ephemeral
  ports inside the test process, so tests run in parallel; the returned
  handles (`FakeGrid`, `FakeAgent`) let the test drive the grid side of
  the conversation (send chat, enqueue CAPS events) and assert on the
  `ServerEvent` stream.
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

See the book chapter "The fake grid" for architecture and usage.
