# Region & Estate Information

A region (a *simulator*, or *sim*) is one square of the grid. Beyond the live
[scene it streams](world.md) — objects, terrain, parcels, avatars — a region has
a body of **descriptive** and **configuration** data: who owns it, how mature
its content is, where it sits on the grid, how many avatars it allows, which
estate it belongs to, and what its sky and water look like. This chapter covers
that data and how to obtain it.

It arrives in four layers, fetched separately:

1. the **identity** every region volunteers on arrival (the handshake);
2. the **limits & settings** you ask for (`RegionInfo`);
3. the **estate** it belongs to (the estate config and access lists);
4. the **environment** — its sky, water, and day cycle (EEP).

## Region identity (the handshake)

When a [circuit](../comms/circuits.md) to a region comes up, the region
introduces itself with a **`RegionHandshake`**. The client replies with
`RegionHandshakeReply` (after which the scene stream begins) and surfaces the
identity as `Event::RegionInfoHandshake(RegionIdentity)`.

`RegionIdentity` carries:

- **name** and **region id** (the region's globally-unique `RegionID`);
- **grid coordinates** — `grid_x` / `grid_y` (the region's index on the world
  map) and the full 64-bit `region_handle` they pack into. The handshake message
  does not itself carry the handle, so it is taken from the handle the session
  already knows for the simulator (seeded from the login response's `region_x` /
  `region_y` for the start region, and otherwise learned from `EnableSimulator`
  and object updates); it is `0` until known;
- **maturity** rating (PG / Mature / Adult) and the **region flags** bitfield
  (both the legacy 32-bit field and the full 64-bit `RegionFlagsExtended`);
- **product** type (Full Region / Homestead / Openspace) and its raw SKU/name;
- the **owner** id, whether *you* are an **estate manager** there, the **water
  height**, the **billing factor**, and the simulator's advertised **CPU class**
  and **CPU ratio** (how many regions share the host).

> The estate-manager flag is about *the current agent* — it gates the estate UI.
> The list of *all* estate managers comes from the estate access lists (below).

## Region limits & settings (`RegionInfo`)

Richer, updatable settings are requested on demand with
`Command::RequestRegionInfo`; the reply is `Event::RegionLimits(RegionLimits)`.
`RegionLimits` adds, beyond what the handshake already told you:

- the **agent capacity** — `max_agents` and the hard agent/object caps (often
  only populated for estate managers, and absent on OpenSim);
- the **estate** ids (`estate_id`, `parent_estate_id`);
- terrain edit limits, the **object bonus factor**, land **price per square
  metre**, and grid redirect coordinates;
- the **sun**: `use_estate_sun` and a fixed `sun_hour`, the region's slice of
  the day cycle (a negative `sun_hour` means the sun cycles normally — for the
  full
  sky/water schedule see [Environment](#environment-eep) below);
- optional **chat** ranges (`RegionInfo5`, newer Second Life only) and
  **combat/damage** settings, each present only when the grid sends its block.

## Estate information

A region belongs to an **estate** — a group of regions sharing an owner, access
lists, and policies. `Command::RequestEstateInfo` asks for the estate config and
its access lists; the replies are:

- `Event::EstateInfo(EstateInfo)` — the estate's **name** and **owner** id;
- `Event::EstateAccessList { estate_id, kind, members }` — one per list, where
  `kind` is the allowed-agents, allowed-groups, banned-agents, or **managers**
  list, and `members` is a list of UUIDs.

The access lists are **ids, not names** — including the estate *managers* list.
To turn those UUIDs (or the region owner, or any other id the protocol hands
you) into something readable, use name resolution.

### Estate covenant

Every estate may publish a **covenant** — a notecard of terms that a buyer
agrees to before purchasing land in it. `Command::RequestEstateCovenant` asks
for the summary; the reply is `Event::EstateCovenant(EstateCovenant)`, carrying
the estate **name** and **owner** id, the covenant's last-changed **timestamp**,
and the covenant notecard's **`covenant_id`**. The covenant text itself is an
asset — fetch it separately with that id when it is non-nil (a nil id means the
estate has no covenant).

### Telehub

A region can route every incoming teleport to a **telehub** — an object with one
or more **spawn points** arrivals are scattered across.
`Command::RequestTelehubInfo` asks for the current layout; the reply is
`Event::TelehubInfo(TelehubInfo)`, with the telehub object's **id** (nil when
the region has none), **name**, **position** and **rotation**, and the list of
**spawn points** (each relative to the telehub).

The telehub is managed by the estate owner (or a god) with four more commands,
each of which is answered by a fresh `Event::TelehubInfo`:

- `Command::ConnectTelehub { object_local_id }` makes an in-region object the
  telehub;
- `Command::DisconnectTelehub` removes the telehub;
- `Command::AddTelehubSpawnPoint { object_local_id }` adds a spawn point at an
  object's position;
- `Command::RemoveTelehubSpawnPoint { spawn_index }` removes a spawn point by
  its zero-based index.

These all travel as `EstateOwnerMessage` `telehub` sub-commands under the hood;
the simulator rejects them unless the agent has estate-owner or god rights.

## Resolving ids to names

The protocol is full of bare UUIDs; names are a separate, on-demand lookup. The
session does **not** resolve or cache names on its own — you ask for the ids you
need and decide what to do with the answers.

- `Command::RequestAvatarNames(ids)` resolves agent ids via `UUIDNameRequest`;
  the reply is `Event::AvatarNames(Vec<AvatarName>)`. Each `AvatarName` has the
  legacy `first_name` / `last_name`, and `legacy_name()` renders them as
  `"First Last"` (collapsing the modern `"Resident"` placeholder to just the
  first name).
- `Command::RequestGroupNames(ids)` resolves group ids via
  `UUIDGroupNameRequest`; the reply is `Event::GroupNames(Vec<GroupName>)`.

Large id lists are split across several requests automatically, and a single
request may be answered by several batched replies.

### Display names

The legacy `First Last` resolved above is an avatar's **immutable** identity.
Second Life layers a **mutable, user-chosen *display name*** over it, resolved
by a separate HTTP [capability](../comms/caps.md) (`GetDisplayNames`) rather
than UDP — so the two are intentionally not conflated.

- `Command::RequestDisplayNames(ids)` batches every agent id into one
  `GetDisplayNames` `GET`; the reply is `Event::DisplayNames(Vec<DisplayName>)`.
  Each `DisplayName` carries the mutable `display_name`, the `username`/SLID
  (e.g. `"james.linden"`), the legacy `legacy_first_name` / `legacy_last_name`
  (with the same `legacy_name()` helper as `AvatarName`), an
  `is_display_name_default` flag (the agent has not set a custom name), and the
  `display_name_expires` / `display_name_next_update` timestamps. Ids the grid
  could not resolve come back as `missing` placeholders.

The capability is Second-Life-centric: stock OpenSim serves `GetDisplayNames`
only when its user-management component is present, and the command is a no-op
when the region seed omits the capability.

## Environment (EEP)

A region's — or an individual parcel's — sky, water, and **day cycle** are the
*Extended Environment* (EEP). Unlike most region data this travels over an HTTP
[capability](../comms/caps.md) (`ExtEnvironment`), not UDP.
`Command::RequestEnvironment { parcel_id }` performs the `GET` (`parcel_id` of
`None` asks for the whole region); the reply is
`Event::Environment(EnvironmentSettings)`.

`EnvironmentSettings` holds the parcel/region id, the **day length** and
**offset** (in seconds), the three **track altitudes** at which the sky changes
with height, and the **day cycle** itself. A `DayCycle` is:

- a set of **tracks** — one for water and up to four for the sky at increasing
  altitudes — each a list of **keyframes**, where a keyframe is a time of day
  (`0.0..=1.0`) and the **name** of a frame to apply; and
- the named **frames** the tracks reference: `SkySettings` (the atmosphere, sun,
  moon, and clouds — colours, densities, rotations, textures, glow, star
  brightness, …) and `WaterSettings` (fog, fresnel, wave directions, the normal
  map, …).

So the day cycle says *when* each named sky/water look applies, and the frames
say *what* each look is. The legacy haze colours and scalars on a sky frame
(`ambient`, `blue_horizon`, the `haze_*` and multiplier values) are read from
its `legacy_haze` block. The deep atmospheric-scattering profiles
(`rayleigh_config`, `mie_config`, `absorption_config`) the renderer uses are not
parsed; every other documented sky/water parameter is.

> **Testing note.** OpenSim ships several of these behind optional modules. Rich
> region/parcel data such as `ParcelProperties` comes through the
> [event queue](../comms/caps.md#the-event-queue-eventqueueget); estate access
> lists, EEP, and the hard agent caps may be empty, absent, or manager-gated
> on a default OpenSim grid. The handshake identity, `RegionInfo` limits, and
> name resolution work on both grids.

---

> **In this codebase**
>
> - Region types are in `sl-proto/src/types/region.rs` (`RegionIdentity`,
>   `RegionLimits`, `RegionChatSettings`, `RegionCombatSettings`); estate types
>   (`EstateInfo`, `EstateAccessKind`, `EstateCovenant`, `TelehubInfo`) in
>   `sl-proto/src/types/map.rs`; legacy
>   name types (`AvatarName`, `GroupName`) in `sl-proto/src/types/name.rs`; the
>   CAPS `DisplayName` in `sl-wire/src/display_name.rs`; environment types
>   (`EnvironmentSettings`, `DayCycle`, `DayCycleFrame`, `SkySettings`,
>   `WaterSettings`) in `sl-proto/src/types/environment.rs`.
> - Commands `RequestRegionInfo`, `RequestEstateInfo`, `RequestEstateCovenant`,
>   `RequestTelehubInfo`, `ConnectTelehub`, `DisconnectTelehub`,
>   `AddTelehubSpawnPoint`, `RemoveTelehubSpawnPoint`, `RequestAvatarNames`,
>   `RequestGroupNames`, `RequestDisplayNames`, `RequestEnvironment` are in
>   `sl-proto/src/command.rs`; the matching events (`RegionInfoHandshake`,
>   `RegionLimits`, `EstateInfo`, `EstateAccessList`, `EstateCovenant`,
>   `TelehubInfo`, `AvatarNames`,
>   `GroupNames`, `DisplayNames`, `Environment`) are in
>   `sl-proto/src/types/event.rs`.
> - The handshake handle and grid coordinates are seeded from the login
>   response (`sl-wire/src/login.rs` `region_x` / `region_y`); a region handle
>   splits into grid coordinates with `handle_to_grid` (and back with
>   `grid_to_handle` / `global_to_handle`).
> - In the [REPL](../tools/sl-repl.md): `request_region_info`,
>   `request_estate_info`, `request_estate_covenant`, `request_telehub_info`,
>   `connect_telehub <object_local_id>`, `disconnect_telehub`,
>   `add_telehub_spawn_point <object_local_id>`,
>   `remove_telehub_spawn_point <spawn_index>`, `request_avatar_names <id…>`,
>   `request_group_names <id…>`, `request_display_names <id…>`, and
>   `request_environment [parcel_id]`.
> - The **server** side mirrors the decoders:
>   `SimSession::send_region_handshake` builds the greeting from a
>   `RegionIdentity`; `send_avatar_names` / `send_group_names` answer the
>   `UUIDNameRequest` the simulator surfaces as
>   `ServerEvent::AvatarNamesRequested` / `GroupNamesRequested`;
>   `build_display_names_response` (with `parse_display_names_query`) builds the
>   `GetDisplayNames` reply body a grid's people service returns;
>   `send_estate_covenant_reply` answers the `EstateCovenantRequest` surfaced as
>   `ServerEvent::RequestEstateCovenant`; `send_telehub_info` answers the
>   telehub `info ui`/management commands (`ServerEvent::RequestTelehubInfo`,
>   `ConnectTelehub`, `DisconnectTelehub`, `AddTelehubSpawnPoint`,
>   `RemoveTelehubSpawnPoint`); and `environment_to_llsd` builds the
>   `ExtEnvironment` reply body.
