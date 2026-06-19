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

Why the teleport is happening is captured by a set of **teleport flags** (via a
landmark, a lure, a login, a telehub, going home, …), which ride along in the
progress and finish notifications.

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
>   `OfferTeleport`, `AcceptTeleportLure`, `DeclineTeleportLure` in
>   `sl-proto/src/command.rs`.
> - The events are `TeleportStarted`, `TeleportProgress`, `TeleportLocal`,
>   `TeleportFinished`, `TeleportFailed`, and `RegionChanged` in
>   `sl-proto/src/types/event.rs`; `TeleportFlags` is in
>   `sl-proto/src/types/`.
> - Neighbour/handover events are `NeighborDiscovered` and `NeighborSeed` in the
>   same `event.rs`; the `Session` (`sl-proto/src/session.rs`) tracks the
>   pending handover and promotes the child circuit.
