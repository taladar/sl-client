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
- **grid coordinates** — `grid_coordinates` (the region's typed
  `GridCoordinates` index pair on the world map) and the full 64-bit
  `region_handle` they pack into. The handshake message
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

## Region telemetry (`SimStats` / `SimulatorTime`)

Two messages the simulator **pushes** unsolicited keep the viewer's idea of the
region current. Neither is requested — they simply arrive.

`SimStats` is the region's periodic **performance telemetry**, pushed roughly
once a second, and surfaces as `Event::SimStats(Box<RegionStats>)`.
`RegionStats` carries the region's `grid_coordinates`, the raw 32-bit
`region_flags` and the full 64-bit `region_flags_extended` (which falls back to
the zero-extended 32-bit field on older simulators that send no `RegionInfo`
block), the
`object_capacity`, and a `stats` list of `(SimStatId, f32)` pairs in the order
the simulator sent them. `SimStatId` is a typed enum over the individual
statistics — time dilation, simulator FPS, physics FPS, agent and active-script
counts, frame times, and so on — whose known ids match both the viewer's
`ESimStatID` and OpenSim's `StatsID` (the two agree on ids 0–40); ids in the
1000+ range are OpenSim-only extras, and any id in neither table is preserved as
`SimStatId::Unknown`.

> Handling `SimStats` is what stops a live session logging it as an unhandled
> message — the original motivation for the message-coverage audit this surface
> came from.

`SimulatorViewerTimeMessage` carries the simulator's **world time and sun
state**, surfaced as `Event::SimulatorTime(Box<SimulatorTime>)`. `SimulatorTime`
holds the microseconds since the simulator started (its monotonic world clock),
the seconds per simulated day and year, and the sun's direction unit vector,
phase angle (radians), and angular velocity. The simulator pushes it so the
viewer can resynchronise its day-cycle clock and sun position against the
[environment](#environment-eep) it is rendering.

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
and the covenant notecard's **`covenant_id`** (an `Option`: `None` when the
estate has no covenant). The covenant text itself is an asset — fetch it
separately with that id when it is `Some`.

### Telehub

A region can route every incoming teleport to a **telehub** — an object with one
or more **spawn points** arrivals are scattered across.
`Command::RequestTelehubInfo` asks for the current layout; the reply is
`Event::TelehubInfo(TelehubInfo)`, with the telehub object's **id** (`None`
when the region has none), **name**, **position** and **rotation**, and the list
of
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

## Terraforming & parcel administration

A land owner (or estate manager) can reshape the region's terrain and adjust a
parcel's settings. These are commands the client *sends*; the simulator enforces
land rights and silently ignores an edit the agent may not make.

**Terraforming.** `Command::ModifyLand(LandEdit)` applies one terraform **brush
stroke**; `Command::UndoLand` reverts the last one. A `LandEdit` bundles:

- the brush **action** — a `LandBrushAction` (`Level`, `Raise`, `Lower`,
  `Smooth`, `Noise`, `Revert`), matching the viewer's `E_LAND_*` codes and the
  `LAND_LEVEL` … `LAND_REVERT` constants LSL's `llModifyLand` exposes;
- the brush **size** — a `LandBrushSize` (`Small` / `Medium` / `Large` = 1 / 2 /
  4 m radius). The radius in metres is what modern simulators read; the legacy
  index byte is still sent for old simulators;
- the **strength** (the wire `Seconds` — how long the brush is held, scaled by
  the configured force; larger values move terrain further per message) and the
  reference **height** the brush levels toward;
- the **area** — a `TerraformArea`, the region-local ground rectangle (west /
  south / east / north metres from the region's south-west corner) the stroke
  covers. The viewer sends a zero-area rectangle at the cursor for click-drag
  brushing (`TerraformArea::point`) and a parcel's bounding rectangle for a
  whole-parcel edit;
- the optional **parcel** being edited (a `RegionLocalParcelId`), or `None` for
  an un-targeted free brush stroke (the wire `LocalID` of `-1`).

**Parcel administration.** Two commands act on a single parcel by its
region-local id (a [`ScopedParcelId`](object-commerce.md), the local id paired
with its region):

- `Command::RequestParcelPropertiesById { local_id, sequence_id }` fetches a
  parcel's properties **by its local id** (in contrast to the rectangle-based
  request); the `sequence_id` is echoed back so the caller can match the reply,
  which arrives as `Event::ParcelProperties`.
- `Command::SetParcelOtherCleanTime { local_id, clean_time }` sets the parcel's
  **auto-return time** for *other* people's objects. `clean_time` is a
  `Duration` rounded to whole minutes; `Duration::ZERO` disables auto-return.

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

## Simulator features (`SimulatorFeatures`)

On arriving in a region the viewer learns what the simulator supports — mesh
upload/rez, the physics-shape types it accepts, attachment and group limits, the
GLTF/PBR-terrain switches — from the `SimulatorFeatures`
[capability](../comms/caps.md), an HTTP `GET` with no UDP equivalent. Both
runtimes fetch it **automatically** once the capability map is known (at login
and on each region change), surfacing the result as
`Event::SimulatorFeatures(Box<SimulatorFeatures>)`;
`Command::RequestSimulatorFeatures` forces an explicit re-fetch.

`SimulatorFeatures` carries the flat flag map (`mesh_upload_enabled`,
`physics_materials_enabled`, `physics_shape_types`, `animated_objects`,
`max_agent_attachments`, `pbr_terrain_enabled`, `gltf_enabled`, …). OpenSim
grids add a nested `OpenSimExtras` subtree — chat ranges
(`say-range`/`shout-range`/ `whisper-range`), the currency symbol, the
map/search/destination-guide URLs, and the prim-scale limits — surfaced in
`open_sim_extras` (it is `None` on Second Life, which omits the map).

## Agent preferences (`AgentPreferences`)

A few preferences live on the grid rather than in the viewer: the avatar's
**hover height**, the **default object permission masks** new objects are
created with, the agent's **maturity-access ceiling**, and the **UI language**
(with a flag for whether it is public in the profile). They are read and written
through the `AgentPreferences` capability, a single HTTP `POST` whose body
carries the fields to change and whose reply echoes the full stored set.

`Command::SetAgentPreferences(Box<AgentPreferences>)` changes only the present
(`Some`) fields; `Command::RequestAgentPreferences` performs a pure "get" (a
`POST` with an empty body). Either way the reply is
`Event::AgentPreferences(Box<AgentPreferences>)`, with every field filled in.

> **Testing note.** OpenSim advertises a subset of `SimulatorFeatures` (with the
> `OpenSimExtras` subtree) and serves `AgentPreferences` through its agent-prefs
> service; Second Life advertises the richer Second-Life-only flags
> (`PBRTerrainEnabled`, `GLTFEnabled`, …) and omits `OpenSimExtras`. Both
> capabilities are guarded on presence in the seed, so the commands are no-ops
> when a grid omits them.

## Resource & physics costs

Several capabilities report how much of a region's budget objects consume — the
numbers behind the build tools' "land impact" readout and the estate-tools "Top
Scripts / Top Colliders" panels. All are guarded on capability presence (so the
commands are no-ops when a grid omits them) and are mostly Second-Life-centric,
though OpenSim serves them too.

- **`GetObjectCost`** — `Command::RequestObjectCost { object_ids }` `POST`s a
  list of object ids; the reply is
  `Event::ObjectCosts(Vec<(ObjectKey, ObjectCost)>)`, each `ObjectCost` carrying
  the
  per-part and whole-linkset *resource* (land-impact) and *physics* costs.
- **`ResourceCostSelected`** — `Command::RequestSelectedCost { object_ids, roots
  }` sums a selection's `physics`/`streaming`/`simulation` cost into one
  `Event::SelectedResourceCost`. `roots` chooses the `selected_roots` (whole
  linksets) vs. `selected_prims` request shape.
- **`GetObjectPhysicsData`** —
  `Command::RequestObjectPhysicsData { object_ids }` fetches each object's
  physics-material parameters (shape type, density, friction, restitution,
  gravity multiplier) as
  `Event::ObjectPhysicsData(Vec<(ObjectKey, ObjectPhysicsData)>)`. The simulator
  also
  **pushes** the same data unsolicited over the [event queue](../comms/caps.md)
  as `ObjectPhysicsProperties` — surfaced as
  `Event::ObjectPhysicsProperties(Vec<(u32, ObjectPhysicsData)>)`, keyed by
  region-local id — when a prim's physics material changes.
- **`AttachmentResources`** — `Command::RequestAttachmentResources` (a `GET`)
  returns `Event::AttachmentResources(Box<AttachmentResourcesReport>)`: the
  agent's scripted attachments grouped by attachment point, plus a
  `ResourceSummary` of available/used memory and URLs.
- **`LandResources`** — `Command::RequestLandResources { parcel_id }` is a
  two-step flow: the `POST` returns follow-up cap URLs
  (`Event::LandResourcesUrls`), which the runtimes then `GET`, surfacing the
  parcel's totals as `Event::LandResourceSummary(ResourceSummary)` and (when the
  agent may see detail) the per-object breakdown as
  `Event::LandResourceDetail(Vec<ParcelScriptResources>)`. The `parcel_id` is
  the region's "fake" parcel id (from a `RemoteParcelRequest` lookup).
- **`LandStatRequest` / `LandStatReply`** — unlike the others this is a UDP
  exchange (estate-manager rights required). `Command::RequestLandStat {
  report_type, request_flags, filter, parcel_local_id }` selects top scripts
  (`LandStatReportType::TopScripts`) or top colliders; the reply is
  `Event::LandStatReply { report_type, request_flags, total_object_count, items
  }`, each `LandStatItem` naming an object, its position, score, and owner.

---

> **In this codebase**
>
> - Region types are in `sl-proto/src/types/region.rs` (`RegionIdentity`,
>   `RegionLimits`, `RegionChatSettings`, `RegionCombatSettings`); estate types
>   (`EstateInfo`, `EstateAccessKind`, `EstateCovenant`, `TelehubInfo`) in
>   `sl-proto/src/types/map.rs`; legacy name types (`AvatarName`, `GroupName`)
>   in `sl-proto/src/types/name.rs`; the CAPS `DisplayName` in
>   `sl-wire/src/display_name.rs`; environment types (`EnvironmentSettings`,
>   `DayCycle`, `DayCycleFrame`, `SkySettings`, `WaterSettings`) in
>   `sl-proto/src/types/environment.rs`; the CAPS `SimulatorFeatures` (with
>   `OpenSimExtras`, `PhysicsShapeTypes`, `AnimatedObjects`) in
>   `sl-wire/src/sim_features.rs` and `AgentPreferences` (with
>   `ObjectPermMasks`) in `sl-wire/src/agent_preferences.rs`. The resource-cost
>   CAPS codecs are in `sl-wire/src/object_cost.rs` (`ObjectCost`,
>   `SelectedResourceCost`), `sl-wire/src/object_physics.rs`
>   (`ObjectPhysicsData`, `PhysicsShapeType`), and
>   `sl-wire/src/resource_report.rs` (`AttachmentResourcesReport`,
>   `ResourceSummary`, `ScriptedObjectInfo`, `LandResourcesUrls`,
>   `ParcelScriptResources`); the UDP `LandStatItem` / `LandStatReportType` are
>   in `sl-proto/src/types/parcel.rs`.
> - Telemetry types `RegionStats`, `SimulatorTime`, and the `SimStatId` enum are
>   in `sl-proto/src/types/region.rs`, surfaced as `Event::SimStats(Box<…>)` and
>   `Event::SimulatorTime(Box<…>)`; both are pushed (no command). The
>   server-side inverses are `SimSession::send_sim_stats` /
>   `send_simulator_time`
>   (`sl-proto/src/sim_session.rs`). In the [REPL](../tools/sl-repl.md) they
>   render as `sim_stats` / `simulator_time`.
> - Terraform types `LandEdit`, `LandBrushAction`, `LandBrushSize`, and
>   `TerraformArea` are in `sl-proto/src/types/land.rs` (each `LandBrush*` type
>   carries the `to_code`/`to_metres`/`to_index` ↔ `from_code`/`from_metres`/
>   `from_index` codec pair). Commands `ModifyLand`, `UndoLand`,
>   `RequestParcelPropertiesById`, and `SetParcelOtherCleanTime`
>   (`sl-proto/src/command.rs`) have `Session` helpers `modify_land`,
>   `undo_land`, `request_parcel_properties_by_id`, and
>   `set_parcel_other_clean_time`
>   (`sl-proto/src/session/methods.rs`); the parcel-by-id reply reuses
>   `Event::ParcelProperties`. The sim side decodes them into
>   `ServerEvent::{ModifyLand, UndoLand, RequestParcelPropertiesById,
>   SetParcelOtherCleanTime}` (`sl-proto/src/sim_session.rs`). REPL tokens:
>   `modify_land`, `undo_land`, `request_parcel_properties_by_id`,
>   `set_parcel_other_clean_time`.
> - Commands `RequestRegionInfo`, `RequestEstateInfo`, `RequestEstateCovenant`,
>   `RequestTelehubInfo`, `ConnectTelehub`, `DisconnectTelehub`,
>   `AddTelehubSpawnPoint`, `RemoveTelehubSpawnPoint`, `RequestAvatarNames`,
>   `RequestGroupNames`, `RequestDisplayNames`, `RequestEnvironment`,
>   `RequestSimulatorFeatures`, `RequestAgentPreferences`,
>   `SetAgentPreferences`, `RequestObjectCost`, `RequestSelectedCost`,
>   `RequestObjectPhysicsData`, `RequestAttachmentResources`,
>   `RequestLandResources`, `RequestLandStat` are in `sl-proto/src/command.rs`;
>   the matching events (`RegionInfoHandshake`, `RegionLimits`, `EstateInfo`,
>   `EstateAccessList`, `EstateCovenant`, `TelehubInfo`, `AvatarNames`,
>   `GroupNames`, `DisplayNames`, `Environment`, `SimulatorFeatures`,
>   `AgentPreferences`, `ObjectCosts`, `SelectedResourceCost`,
>   `ObjectPhysicsData`, `ObjectPhysicsProperties`, `AttachmentResources`,
>   `LandResourcesUrls`, `LandResourceSummary`, `LandResourceDetail`,
>   `LandStatReply`) are in `sl-proto/src/types/event.rs`. The two-step
>   `LandResources` flow lives in the runtimes (`fetch_land_resources` /
>   `run_land_resources`), which `GET` the follow-up URLs and forward them under
>   the `LAND_RESOURCE_SUMMARY_TAG` / `LAND_RESOURCE_DETAIL_TAG` tags.
> - The handshake handle and grid coordinates are seeded from the login
>   response (`sl-wire/src/login.rs` `region_x` / `region_y`); a region handle
>   splits into grid coordinates with `handle_to_grid` (and back with
>   `grid_to_handle` / `global_to_handle`).
> - In the [REPL](../tools/sl-repl.md): `request_region_info`,
>   `request_estate_info`, `request_estate_covenant`, `request_telehub_info`,
>   `connect_telehub <object_local_id>`, `disconnect_telehub`,
>   `add_telehub_spawn_point <object_local_id>`,
>   `remove_telehub_spawn_point <spawn_index>`, `request_avatar_names <id…>`,
>   `request_group_names <id…>`, `request_display_names <id…>`,
>   `request_environment [parcel_id]`, `request_simulator_features`,
>   `request_agent_preferences`, `set_agent_preferences [hover_height=]
>   [perm_group=] [perm_everyone=] [perm_next_owner=] [max_access=PG|M|A]
>   [language=] [language_is_public=]`, `request_object_cost <id…>`,
>   `request_selected_cost <id…> [roots=true]`,
>   `request_object_physics_data <id…>`, `request_attachment_resources`,
>   `request_land_resources <parcel_id>`, and `request_land_stat
>   [report_type=scripts|colliders] [request_flags] [filter] [parcel_local_id]`.
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
>   `ExtEnvironment` reply body. `build_simulator_features_response` and
>   `build_agent_preferences_response` (with `parse_agent_preferences`) build
>   the `SimulatorFeatures` / `AgentPreferences` cap reply bodies a grid
>   returns. The resource-cost CAPS expose matching reply builders
>   (`build_get_object_cost_response`, `build_resource_cost_selected_response`,
>   `build_get_object_physics_data_response`, `build_object_physics_properties`,
>   `build_attachment_resources_response`, `build_land_resources_response`,
>   `build_land_resource_summary_response`,
>   `build_land_resource_detail_response`), and
>   `SimSession::send_land_stat_reply` answers a `LandStatRequest`.
