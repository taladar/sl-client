# Teleport

Teleport is how an avatar moves somewhere it cannot simply walk. It spans two
quite different cases — staying inside the current region, and handing the
avatar over to a different region's process — and the second case is where the
[multi-circuit](../comms/circuits.md#multiple-circuits-and-region-crossing)
machinery earns its keep.

## Requesting a teleport

A client asks to teleport with a single command carrying the destination region
and the position/orientation within it. There are a few flavours:

- **Direct teleport** to a region handle + local position (`Command::Teleport`).
- **Teleport on request** to another agent (`Command::RequestTeleport`).
- **Lure** handling: you can offer another avatar a teleport
  (`Command::OfferTeleport`) and accept or decline an offered lure
  (`Command::AcceptTeleportLure` / `Command::DeclineTeleportLure`). Like a
  landmark teleport the destination is resolved simulator-side. OpenSim packs
  the offerer's region handle and position into the lure id (a *fake parcel
  id*, `sl_wire::FakeParcelId`), which gives an early destination hint; Second
  Life's lure id is opaque, so there is none and the handle only becomes known
  when the teleport finishes.
- **Landmark teleport** (`Command::TeleportViaLandmark { landmark }`) teleports
  to a landmark inventory item's *asset* id; a `landmark` of `None` teleports to
  the agent's **home** location. Unlike a direct teleport the destination is
  resolved simulator-side, so the region handle only becomes known when the
  teleport finishes.
- **Cancel** (`Command::CancelTeleport`) aborts a teleport already in progress;
  the session reverts to its prior active state.

Why the teleport is happening is captured by a set of **teleport flags** (via a
landmark, a lure, a login, a telehub, going home, …), which ride along in the
progress and finish notifications.

## Setting your home / start location

`Command::SetStartLocation { slot, position, look_at }` records a start
location (`SetStartLocationRequest`): it stores the region-local `position` and
`look_at` direction under a `StartLocationSlot`. The everyday use is
`StartLocationSlot::Home` — "set home to here" — but the slot also names the
viewer's other `EStartLocation` ordinals (`Last`, `Direct`, `Parcel`,
`Telehub`, `Url`).

> The login-time `start=` parameter is a *different* type — the SLURL-style
> [`StartLocation`](login.md) (`last` / `home` / `uri:Region&x&y&z`) that says
> *where to log in*. `StartLocationSlot` is the wire `LocationID` of the request
> that *records* a slot, and the two are kept deliberately distinct.

## Related agent commands

Three small session commands ride alongside the teleport surface (none has a
reply event):

- `Command::RequestAgentDataUpdate` polls for a fresh `AgentDataUpdate` (the
  active group / title / name data) without changing anything
  (`AgentDataUpdateRequest`).
- `Command::QuitCopy` logs out while *leaving the agent's in-world objects
  behind* (`AgentQuitCopy`), reusing the circuit's own code.
- `Command::SetVelocityInterpolation { enabled }` toggles simulator-side
  velocity interpolation of object motion (`VelocityInterpolateOn` /
  `VelocityInterpolateOff`).

## The event sequence

A teleport is reported as a small sequence of events, and the branch it takes
tells you whether it was local or a region handover:

```text
TeleportStarted
   └─ TeleportProgress { message, flags }      (zero or more updates)
        ├─ TeleportLocal                        → same region, same circuit. Done.
        ├─ TeleportFinished { region_handle, sim, maturity, flags }
        │     └─ RegionChanged { region_handle, sim }   → arrived in a new region
        └─ TeleportFailed { reason, alert_info }        → it did not happen
```

- **Local teleport** (`Event::TeleportLocal`) is the easy case: the destination
  is in the region you are already connected to, so the existing circuit is
  reused and the avatar is simply repositioned.
- **Cross-region teleport** ends with `Event::TeleportFinished` followed by
  `Event::RegionChanged`: a new circuit to the destination simulator has become
  the root, and the destination's
  [region handshake](world.md#the-region-handshake) and a fresh
  [capability](../comms/caps.md) seed follow.
- **Failure** (`Event::TeleportFailed`) carries a reason and any region alert
  text.

## Cross-region handover and child circuits

A region **announces its neighbours**, so a client is already connected to the
regions around it before it ever steps across a border:

- an **`EnableSimulator`** message and/or an **`EstablishAgentCommunication`**
  event give the neighbour's address and **seed capability** (surfaced here as
  `Event::NeighborSeed { sim, seed_capability }` and
  `Event::NeighborDiscovered`),
- the client POSTs that seed to establish a **child circuit**, and
- on a crossing — or a teleport *into that neighbour* — the child circuit is
  promoted to the root circuit.

A **distant** teleport is not the long-distance version of that. Its
destination is announced by nothing: `TeleportFinish` carries the destination's
address and seed itself, and the client opens the circuit off the back of it
(`UseCircuitCode` + `CompleteAgentMovement`). OpenSim's `TransferAgent_V2` says
so where it sends the finish — *"New protocol: send TP Finish directly, without
prior ES or EAC. That's what happens in the Linden grid"* — and the reference
viewer's `process_teleport_finish` matches, sending `UseCircuitCode` to the
address the finish names whether or not it already holds that region. Only the
legacy `TransferAgent_V1`, taken when the destination speaks a simulator
protocol older than 0.2, puts an `EnableSimulator` +
`EstablishAgentCommunication` in front of the finish.

That difference is what `Event::RegionChanged`'s **`world_reset`** flag reads.
A destination that is already a child circuit, or positionally adjacent, keeps
the world and merely re-bases it; anything else replaces it, and the flag tells
the viewer to purge every store scoped to the world it just left. A grid that
announced its teleport destinations would make every destination look like a
neighbour and the flag would never fire — which is exactly what `sl-fake-grid`
did until it was corrected; the roadmap task
`viewer-teleport-never-resets-the-world` has the whole story.

> **Practical note.** True cross-region teleport requires holding child-agent
> circuits to the destination. The `sl-survey` tool sidesteps this: rather than
> teleporting region to region, it traverses the grid by re-logging-in directly
> at each region (via the map), which is simpler for a headless crawler that
> does not need continuity of presence.

## The server side

The sans-I/O `SimSession` provides the simulator half as **mechanics the
driver sequences** — deliberately without a sim-side teleport phase
machine, because one session cannot know whether an inter-region
teleport succeeds: the destination is *another* `SimSession`, and only
the driver (a fake grid, a test) sees both.

A request surfaces as a typed event (`TeleportRequested` with handle,
position and look-at; `TeleportViaLandmark`; `TeleportViaLure`;
`CancelTeleport`). The driver then strings together:

- **UDP mechanics** on the source circuit: `send_teleport_start`,
  `send_teleport_progress`, and either `send_teleport_local` (an
  intra-region finish — no circuit change) or `send_teleport_failed`
  (back to active with a reason).
- **Event-queue mechanics** on the source's CAPS event queue:
  `enqueue_teleport_finish` finishes an inter-region teleport on its own
  (the client opens the destination's circuit and sends
  `CompleteAgentMovement` on it), and `enqueue_crossed_region` is the
  border-crossing variant (no teleport screen).
- **Neighbour announcement**, which is a *separate* concern that happens
  on arrival rather than on teleport: `enqueue_enable_simulator` (the
  client opens a *child* circuit) and
  `enqueue_establish_agent_communication` (that child's seed capability —
  this event has **no** UDP form). Putting these in front of a teleport's
  finish is the legacy `TransferAgent_V1` shape, and it hides a distant
  destination behind a neighbour's face; see [Cross-region handover and
  child circuits](#cross-region-handover-and-child-circuits).

The destination `SimSession` tracks **agent presence**: a circuit opened
by `UseCircuitCode` alone hosts a *child* agent
(`AgentPresence::Child`), and `CompleteAgentMovement` promotes it to the
*root* agent — exactly how the region servers distinguish the two. The
source retires its now-child circuit with `send_disable_simulator`.

The two-`SimSession` loopback test drives a real client `Session`
through the whole sequence — request, child circuit, finish, promotion,
arrival confirmation, teardown — over the real wire path plus the real
event-queue serialization.

---

> **In this codebase**
>
> - Teleport commands are `Command::Teleport`, `RequestTeleport`,
>   `OfferTeleport`, `AcceptTeleportLure`, `DeclineTeleportLure`,
>   `TeleportViaLandmark`, `CancelTeleport`, and `SetStartLocation` in
>   `sl-proto/src/command.rs` (helpers `teleport_via_landmark`,
>   `cancel_teleport`, `set_start_location`); `StartLocationSlot` (with
>   `to_code`/`from_code`) is in `sl-proto/src/types/session.rs`. The related
>   agent commands `RequestAgentDataUpdate`, `QuitCopy`, and
>   `SetVelocityInterpolation` (helpers `request_agent_data_update` /
>   `quit_copy` / `set_velocity_interpolation`) live there too.
> - Server events: the sim side decodes these into
>   `ServerEvent::{TeleportViaLandmark, CancelTeleport, SetStartLocation,
>   RequestAgentDataUpdate, QuitCopy, SetVelocityInterpolation}`
>   (`sl-proto/src/sim_session.rs`); REPL tokens `teleport_via_landmark`,
>   `cancel_teleport`, `set_start_location`, `request_agent_data_update`,
>   `quit_copy`, `set_velocity_interpolation`.
> - The events are `TeleportStarted`, `TeleportProgress`, `TeleportLocal`,
>   `TeleportFinished`, `TeleportFailed`, and `RegionChanged` in
>   `sl-proto/src/types/event.rs`; `TeleportFlags` is in
>   `sl-proto/src/types/`.
> - Neighbour/handover events are `NeighborDiscovered` and `NeighborSeed` in the
>   same `event.rs`; the `Session` (`sl-proto/src/session.rs`) tracks the
>   pending handover and promotes the child circuit.
> - Server side (`sl-proto/src/sim_session.rs`):
>   `ServerEvent::{TeleportRequested, TeleportViaLure}` (beside the existing
>   landmark/cancel events), the `send_teleport_*` /
>   `send_disable_simulator` mechanics, the `enqueue_enable_simulator` /
>   `enqueue_establish_agent_communication` / `enqueue_teleport_finish` /
>   `enqueue_crossed_region` event-queue wrappers, and
>   `AgentPresence` (`agent_presence()` / `is_root_agent()`). The two-sim
>   loopback tests are `inter_region_teleport_two_sims` and
>   `crossed_region_two_sims` in `sl-proto/tests/sim_session.rs`.
