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

Extended, less-frequently-changing data — creator, full permissions, name,
description, sale info — comes separately as **object properties**, requested on
demand.

Object updates also carry a **time dilation** value: when a region is overloaded
it runs physics slower than real time, and the dilation lets the client
interpolate correctly.

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

---

> **In this codebase**
>
> - Object-update codecs are in `sl-proto/src/object_update/` (`full.rs`,
>   `compressed.rs`, `terse.rs`); the `Object`, `ObjectMotion`,
>   `ObjectProperties`, `TextureEntry`, and extra-parameter types are in
>   `sl-proto/src/types/object.rs` (extra params in
>   `sl-proto/src/extra_params.rs`, particles in `sl-proto/src/particles.rs`).
>   Events: `ObjectAdded`, `ObjectUpdated`, `ObjectRemoved`, `ObjectProperties`,
>   `TimeDilation`.
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
