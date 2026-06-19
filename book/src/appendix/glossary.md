# Glossary

Short definitions for the identifiers and jargon that recur throughout the book.

**Agent** — an avatar as a network participant. The *agent id* is the avatar's
permanent UUID.

**Agent ID** — the avatar's stable UUID identity, the same across every
[session](../comms/sessions.md).

**AIS / AIS3** — the *Avatar Inventory Service* HTTP API (version 3), the modern
[CAPS](../comms/caps.md)-based way to read and write
[inventory](../content/inventory.md).

**Asset** — the underlying data of something (a texture, sound, animation, mesh,
script). Referenced by an *asset id*; an [inventory](../content/inventory.md)
item *points at* an asset.

**Capability (CAP)** — an unguessable, per-session HTTPS URL granting access to
one server feature. See [CAPS](../comms/caps.md).

**Circuit** — one agent's UDP connection to one simulator. See
[Circuits](../comms/circuits.md).

**Circuit code** — the 32-bit integer (issued at [login](../content/login.md))
that authorizes a circuit.

**Event queue** — the `EventQueueGet` long-poll that delivers asynchronous
server events. See [CAPS](../comms/caps.md#the-event-queue-eventqueueget).

**Full id** — an object's global UUID, as opposed to its region-local
*local id*.

**Grid** — a whole world: a login service plus many regions. Second Life and
OpenSim are different grids speaking nearly the same protocol.

**Local id** — a compact, per-region integer handle for an object, reused within
a region. Real-time updates refer to objects by local id.

**LLSD** — *Linden Lab Structured Data*, the JSON-like data format used over
HTTP. See [LLSD](../comms/llsd.md).

**LLUDP** — the *Linden Lab UDP* protocol: framing and reliability over UDP. See
[LLUDP Transport](../comms/lludp-transport.md).

**Maturity** — a region/parcel content rating (General / Moderate / Adult).

**Message** — a typed LLUDP packet body, defined in the
[message template](../comms/messages.md).

**Message template** — `message_template.msg`, the shared file defining every
LLUDP message. See [Messages & the Template](../comms/messages.md).

**Parcel** — a subdivision of a region's land with its own ownership and rules.
See [3D World Information](../content/world.md#parcels).

**PCODE** — the primitive code classifying an object: primitive, avatar, grass,
tree, particle system.

**Region** (also *simulator*, *sim*) — the server process owning one square of
the world.

**Region handle** — a 64-bit value encoding a region's global grid position,
used to name a region in [teleport](../content/teleport.md) and map operations.

**Sans-I/O** — the design where protocol logic does no networking itself; see
[Architecture](../architecture.md).

**Seed capability** — the single capability URL returned at login, from which
all other capabilities are fetched. See
[CAPS](../comms/caps.md#the-seed-capability).

**Session** — one logged-in presence of one avatar. See
[Sessions](../comms/sessions.md).

**Session ID** — the per-login UUID that, with the agent id, authenticates
messages.

**Texture entry** — the per-face texture/colour description attached to an
object.

**Zero-coding** — the run-length compression of zero bytes in a message body
when the `ZEROCODED` flag is set. See
[LLUDP Transport](../comms/lludp-transport.md#zero-coding).
