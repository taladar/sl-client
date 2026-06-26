# 3D World Information

This is the heart of what a region streams to a client: the **objects** in the
scene, the **terrain** under them, the **parcels** the land is divided into, and
the **avatars** present. It is also the highest-volume traffic, so it leans
hardest on [LLUDP](../comms/lludp-transport.md) and on compact encodings.

## Objects

Everything physical in the world — prims, linked builds, trees, particle
emitters, and avatars too — is an **object**. The region announces and updates
objects with a family of messages tuned for different change rates:

- **Full object update** (`ObjectUpdate`) — the complete state: ids, the parent
  link, the *pcode* (primitive / avatar / grass / tree / particle), the
  transform (position, velocity, acceleration, rotation, angular velocity), the
  shape, the texture entry (per-face textures and colours), and extra parameters
  (flexi, light, sculpt, reflection probe, …). Sent when an object first appears
  or changes substantially.
- **Compressed object update** (`ObjectUpdateCompressed`) — the same information
  in a packed binary block, for efficiency.
- **Terse object update** (`ImprovedTerseObjectUpdate`) — *motion only*: just
  the changing position/velocity/rotation, no shape or textures. This is the
  high-frequency path that keeps moving things smooth.

Each update carries a **local id** (a per-region handle, compact and reused) as
well as the object's **full id** (its global UUID). Real-time traffic refers to
objects by local id; persistent references use the full id.

A local id is only meaningful *within the one circuit* it was learned on: the
scene cache is partitioned per circuit, the simulator recycles ids as objects
come and go, and a reconnect (even to the same region) starts a fresh id space.
The caller-facing API therefore hands back and accepts a **`ScopedObjectId`** (a
`ScopedParcelId` for parcels) — the local id paired with a `CircuitId`, the
client-minted identity of that circuit instance. Build one from a cached
`Object::scoped_id()`, or from `Session::root_circuit_id()` plus a raw id; the
object/parcel `Session` methods resolve it back to the live circuit and return
`Error::UnknownCircuit` for a stale one, so an id captured in one region (or
before a reconnect) can never silently act on another. The wire codec still
carries only the bare local id — the circuit scope is a client-side concern and
is never serialized.

Extended, less-frequently-changing data — creator, full permissions, name,
description, sale info — comes separately as **object properties**, requested on
demand.

Object updates also carry a **time dilation** value: when a region is overloaded
it runs physics slower than real time, and the dilation lets the client
interpolate correctly.

### Object animations

An **animated-mesh** (animesh) object can play skeletal animations of its own,
driven by `llStartObjectAnimation`. The simulator pushes the object's current
animation set as `Event::ObjectAnimation { object_id, animations }`, the object
analogue of an avatar's [`AvatarAnimation`](#avatars-in-the-region). Like that
event, the list is the *complete* authoritative set now playing — an animation
that stops simply drops out of a later update — not a delta. Each
`ObjectPlayingAnimation` pairs the animation's `anim_id` (an `AnimationKey`)
with its `sequence_id`.

### Editing objects

A client with build rights reshapes a prim by sending edit messages that target
it by [`ScopedObjectId`](#objects). Three rewrite the parts of an object that a
full update carries:

- `Command::SetObjectShape { local_id, shape }` sets the path/profile geometry —
  the quantized `PrimShapeParams` that an `ObjectUpdate` decodes to.
- `Command::SetObjectImage { local_id, media_url, texture_entry }` sets the
  per-face textures and colours (a `TextureEntry`), plus the legacy parcel-media
  URL (`None` for none).
- `Command::SetObjectExtraParams { local_id, params }` sets the *complete*
  extra-parameter state (flexi, light, sculpt, …). It is wholesale: any subtype
  **absent** from `params` is cleared, so `ObjectExtraParams::default()` strips
  every extra parameter from the prim.

These join the other object-edit commands (material, flags, group, …); the
simulator confirms each the usual way, by pushing the changed object's
`ObjectUpdate`.

## The region handshake

When a [circuit](../comms/circuits.md) to a region comes up, the region
introduces itself with a **`RegionHandshake`** carrying its *identity*: name,
region flags, maturity rating, product type, owner, water height, billing
factor, and whether you are an estate manager there. The client replies with
`RegionHandshakeReply`, after which the scene/terrain stream begins. Richer,
updatable region settings (`RegionInfo`) can be requested afterward.

The descriptive and configuration data of a region — its identity, agent
limits, estate, and sky/water environment — is covered on its own in
[Region & Estate Information](region.md); this chapter is about the live scene.

## Terrain

The ground is a heightfield, delivered as compressed **terrain patches** — small
square tiles of height data encoded with a frequency transform. The client
assembles patches into the region's terrain and re-applies updates as the land
is edited. There are also terrain *texture/material* layers describing how the
heightfield is painted.

## Parcels

A region's land is subdivided into **parcels**, each with its own ownership,
rules, and media. A parcel's data includes its geometry (an axis-aligned
bounding box and an ownership bitmap over the region grid), ownership (owner,
group, group owned, status — leased / pending / abandoned), prim limits, dwell,
access lists, and [media](sound-media.md) settings.

A protocol subtlety: while there are UDP messages for parcels, the **rich parcel
data (`ParcelProperties`) is delivered through the
[event queue](../comms/caps.md#the-event-queue-eventqueueget)**, not over UDP.
A client requests it (`Command::RequestParcelProperties`) and receives
`Event::ParcelProperties` from the event-queue side. Overlay info (the colored
parcel grid) and dwell come back separately.

### Parcel management

Beyond reading a parcel, a client with land rights can **manage** it over UDP:

- **edit** its settings (`Command::UpdateParcel`), **buy** / **deed** /
  **reclaim** / **release** it, and edit its **access** and **ban** lists,
- **join** several owned, leased parcels inside a metre rectangle into one
  (`Command::JoinParcels`) or **divide** a rectangle out of a parcel into a new
  one (`Command::DivideParcel`),
- list **who owns objects** on the parcel
  (`Command::RequestParcelObjectOwners` → `Event::ParcelObjectOwners`, one row
  per owner with a count and online flag),
  and **return** or **disable** those objects
  (`Command::ReturnParcelObjects` / `Command::DisableParcelObjects`, scoped by
  owner/group/other or an explicit id list),
- and **buy a temporary access pass** to a restricted parcel
  (`Command::BuyParcelPass`).

The places/search panels identify a parcel by a grid-wide **parcel id** rather
than a region-local id. A client resolves that id from a region location through
the `RemoteParcelRequest` capability (`Command::RequestRemoteParcelId` →
`Event::RemoteParcelId`), then fetches the parcel's basic listing — name,
description, area, owner, sale price, dwell, global position —
over UDP (`Command::RequestParcelInfo` → `Event::ParcelDetails`). This is
distinct from the rich `ParcelProperties` above: it is the condensed, by-id form
the search results show.

## Avatars in the region

Other avatars are simply objects with the **avatar** pcode, so they arrive
through the same object-update stream and move via terse updates. Layered on
top:

- **appearance** — the baked textures and worn items that determine how an
  avatar looks (`Event::AvatarAppearance`; see the [Appearance](appearance.md)
  chapter),
- **animations** — which animations an avatar is currently playing
  (`Event::AvatarAnimation`),
- and the usual name/identity data.

## The world map

The world map is assembled from three separate queries, all sent to the current
region's circuit and answered over UDP:

- **Map blocks** — `Command::RequestMapBlocks` (a grid-coordinate rectangle) or
  `RequestMapByName` (search by name) ask for the per-region details: name, grid
  coordinates, maturity, size, and the region's map-tile texture id. Each region
  arrives as an `Event::MapBlock`.
- **Map items** — `Command::RequestMapItems` asks for a map overlay of a given
  kind (avatar "green dots", telehubs, land for sale, events). They arrive as an
  `Event::MapItems` carrying global-coordinate `MapItem`s.
- **Map layers** — `Command::RequestMapLayer` asks for the zoomed-out image
  tiles: each `MapLayer` (in the resulting `Event::MapLayers`) gives a texture
  and the inclusive grid rectangle (`left..=right` by `bottom..=top`) it covers.
  The viewer stitches these tiles into the background of the world map, then
  overlays the per-region detail from the map blocks. Second Life's main grid is
  a single global layer; OpenSim grids report their own coverage.

## Reporting abuse & filing postcards

Two outbound, fire-and-forget viewer actions reach the grid here:

- **Abuse / bug reports** — the "Report Abuse" floater gathers a complaint
  (the abuser, the offending object, a summary and free-text details, a
  snapshot, and the region the abuse happened in) and sends it. Second Life
  prefers the `SendUserReport` capability (`Command::SendAbuseReportViaCaps`, an
  LLSD POST), falling back to the legacy `UserReport` UDP message
  (`Command::SendAbuseReport`); OpenSim implements only the UDP path. Either way
  there is no reply. When `SendAbuseReportViaCaps` carries a `screenshot` and
  the region offers the `SendUserReportWithScreenshot` capability, the runtime
  first uploads the snapshot over that cap's two-step uploader (the same
  `{ state, uploader, … }` flow as `NewFileAgentInventory`), fills the report's
  `screenshot_id` with the new texture asset id, and completes the report
  referencing it — mirroring the viewer's `sendReportViaCaps`. With no
  screenshot (or on a grid without the cap) the plain `SendUserReport` POST is
  used.
- **Postcards** — `Command::SendPostcard` emails a snapshot (already uploaded as
  an asset) to one or more addresses with a subject and message, optionally
  asking the grid to publish it on its web gallery. Fire-and-forget.

## Simulator notifications

Beyond world geometry, the simulator pushes a handful of receive-only
notifications a viewer surfaces directly to the user or its HUD:

- **Alerts** — a general `AlertMessage` carries an already-localized string to
  show the user, optionally with structured `AlertInfo` keys (a message key the
  viewer looks up in its `alerts.xml` for localization, plus substitution
  parameters) and the agents the alert targets. An `AgentAlertMessage` is the
  same thing addressed to one specific agent, with a `modal` flag asking the
  viewer to block on a dialog.
- **Mean collisions** — a `MeanCollisionAlert` reports avatar-on-avatar
  collisions (the data behind the viewer's "Bumps, Pushes & Hits" panel): for
  each, the victim, the perpetrator, when it happened, the magnitude, and how it
  occurred (a bump, an `llPushObject`, or a dragged/scripted/physical object).
- **Health** — a `HealthMessage` reports the agent's current health in a
  damage-enabled region (`100.0` is full; `0.0` sends the agent home).
- **Camera constraint** — a `CameraConstraint` hands the viewer a collision
  plane `[nx, ny, nz, d]` so it can keep the third-person camera from clipping
  into an obstruction.
- **Viewer freeze** — a `ViewerFrozenMessage` carries a single boolean telling
  the viewer it has been frozen (`true`) or thawed (`false`) by an estate
  manager or parcel owner (the estate-tools freeze, the parcel "freeze"
  option, or `llFreezeAvatar`). The freeze is enforced **server-side**: the
  simulator stops processing the agent's movement input, so walking, flying,
  turning, and in-world interaction are all suppressed until it thaws (the
  reference viewer's handler is a no-op — the message is purely informational,
  letting the viewer show the user why their controls stopped responding).
  Teleporting is **not** blocked: a teleport request is a separate message path
  the freeze does not gate, so teleporting away (or relogging) is the usual way
  out of a freeze, which is region-local.

None of these has a reply; the client just acts on them.

---

> **In this codebase**
>
> - Object-update codecs are in `sl-proto/src/object_update/` (`full.rs`,
>   `compressed.rs`, `terse.rs`); the `Object`, `ObjectMotion`,
>   `ObjectProperties`, `TextureEntry`, and extra-parameter types are in
>   `sl-proto/src/types/object.rs` (extra params in
>   `sl-proto/src/extra_params.rs`, particles in `sl-proto/src/particles.rs`).
>   Events: `ObjectAdded`, `ObjectUpdated`, `ObjectRemoved`, `ObjectProperties`,
>   `TimeDilation`. Animesh animation pushes are
>   `Event::ObjectAnimation { object_id, animations }` carrying
>   `ObjectPlayingAnimation` (`anim_id` / `sequence_id`); the sim-side inverse
>   is `SimSession::send_object_animation`.
> - Object editing: `Command::SetObjectShape` / `SetObjectImage` /
>   `SetObjectExtraParams` have `Session` helpers `set_object_shape` /
>   `set_object_image` / `set_object_extra_params`
>   (`sl-proto/src/session/methods.rs`), reusing the inbound `PrimShapeParams` /
>   `TextureEntry` / `ObjectExtraParams` domain types. The simulator decodes
>   them into `ServerEvent::ObjectShapeSet` / `ObjectImageSet` /
>   `ObjectExtraParamsSet` (`sl-proto/src/sim_session.rs`); REPL tokens
>   `set_object_shape` / `set_object_image` / `set_object_extra_params`.
> - The region handshake yields `Event::RegionHandshakeComplete` then
>   `Event::RegionInfoHandshake(RegionIdentity)`
>   (`sl-proto/src/types/region.rs`); `Command::RequestRegionInfo` fetches
>   updatable settings (`RegionInfoUpdate`).
> - Terrain is `sl-proto/src/terrain.rs` (`TerrainPatch`, `encode_layer`),
>   surfaced as `Event::TerrainPatch`.
> - Parcels are `sl-proto/src/types/parcel.rs` (`ParcelInfo`, `ParcelStatus`,
>   `ParcelCategory`, `LandingType`, plus the by-id `ParcelDetails` and
>   per-owner `ParcelObjectOwner`); request via
>   `Command::RequestParcelProperties`, receive `Event::ParcelProperties` /
>   `ParcelOverlay` / `ParcelDwell`.
> - Parcel management (`JoinParcels`, `DivideParcel`,
>   `RequestParcelObjectOwners`, `BuyParcelPass`, `DisableParcelObjects`,
>   `RequestParcelInfo`) maps to the UDP encoders in
>   `sl-proto/src/session/circuit.rs`; the simulator side decodes each into a
>   `ServerEvent` and answers `RequestParcelObjectOwners` / `RequestParcelInfo`
>   with `SimSession::send_parcel_object_owners_reply` /
>   `send_parcel_info_reply`.
> - `RequestRemoteParcelId` posts the `RemoteParcelRequest` capability
>   (`sl-wire/src/remote_parcel.rs`), decoded into `Event::RemoteParcelId`.
> - The world map's three queries are `Command::RequestMapBlocks` /
>   `RequestMapByName` (→ `Event::MapBlock`, `MapRegionInfo`),
>   `RequestMapItems` (→ `Event::MapItems`, `MapItem`/`MapItemType`), and
>   `RequestMapLayer` (→ `Event::MapLayers`, `MapLayer`) — types in
>   `sl-proto/src/types/map.rs`, UDP encoders in
>   `sl-proto/src/session/circuit.rs`. The simulator side decodes the four
>   request messages into `ServerEvent::MapBlockRequested` /
>   `MapNameRequested` / `MapItemRequested` / `MapLayerRequested` (carrying the
>   requested rectangle / name / item type / region handle, plus the map-layer
>   flags) and answers with `SimSession::send_map_block_reply` /
>   `send_map_item_reply` / `send_map_layer_reply`.
> - Abuse reports use `sl-wire/src/abuse_report.rs` (`AbuseReport`,
>   `AbuseReportType`): `Command::SendAbuseReport` encodes the `UserReport` UDP
>   message; `Command::SendAbuseReportViaCaps { report, screenshot }` posts the
>   `SendUserReport` capability (`build_send_user_report`), or — when
>   `screenshot` is present and the region offers `SendUserReportWithScreenshot`
>   (`CAP_SEND_USER_REPORT_WITH_SCREENSHOT`) — runs that cap's two-step uploader
>   (`run_report_screenshot_upload`) to attach the snapshot first. The simulator
>   decodes the UDP form into `ServerEvent::AbuseReportReceived`.
> - Postcards are `sl-proto/src/types/report.rs` (`Postcard`):
>   `Command::SendPostcard` encodes the `SendPostcard` UDP message, decoded on
>   the simulator side into `ServerEvent::PostcardReceived`.
> - Simulator notifications are receive-only: `AlertMessage`,
>   `AgentAlertMessage`, `MeanCollisionAlert`, `HealthMessage`,
>   `CameraConstraint`, and `ViewerFrozenMessage` decode into the same-named
>   `Event`s (`ViewerFrozenMessage` → `Event::ViewerFrozen { frozen }`; the
>   `AlertInfo` type is in `sl-proto/src/types/script.rs`; `MeanCollision` /
>   `MeanCollisionType` in `sl-proto/src/types/alert.rs`). The simulator side
>   emits them with `SimSession::send_alert_message`,
>   `send_agent_alert_message`, `send_mean_collision_alert`,
>   `send_health_message`, `send_camera_constraint`, and `send_viewer_frozen`.
