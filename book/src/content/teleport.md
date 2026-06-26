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
  (`Command::AcceptTeleportLure` / `Command::DeclineTeleportLure`).
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

For a cross-region teleport (and for ordinary border crossings), the destination
must be reachable before the avatar arrives. The current region announces the
destination so the client can pre-establish a connection:

- an **`EnableSimulator`** message and/or an **`EstablishAgentCommunication`**
  event give the neighbour/destination's address and **seed capability**
  (surfaced here as `Event::NeighborSeed { sim, seed_capability }` and
  `Event::NeighborDiscovered`),
- the client POSTs that seed to establish a **child circuit**, and
- on the teleport finishing, that child circuit is promoted to the root circuit.

This is the same mechanism that lets an avatar see and step into a neighbouring
region seamlessly — a deliberate teleport is just the long-distance version.

> **Practical note.** True cross-region teleport requires holding child-agent
> circuits to the destination. The `sl-survey` tool sidesteps this: rather than
> teleporting region to region, it traverses the grid by re-logging-in directly
> at each region (via the map), which is simpler for a headless crawler that
> does not need continuity of presence.

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
