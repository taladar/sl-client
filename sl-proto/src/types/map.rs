//! Estates, region info updates, and world-map items.

use std::net::SocketAddr;

use super::Maturity;
use sl_types::key::{ObjectKey, TextureKey};
use sl_types::lsl::Rotation;
use sl_types::lsl::Vector;
use sl_types::map::{GridCoordinates, GridRectangle, RegionCoordinates, RegionName};
use sl_wire::{GlobalCoordinates, RegionHandle};
use uuid::Uuid;

/// A change to one of an estate's access lists, applied via
/// [`Session::update_estate_access`](crate::Session::update_estate_access)
/// (`EstateOwnerMessage` method `estateaccessdelta`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EstateAccessDelta {
    /// Add an agent to the allowed-access list.
    AllowedAgentAdd,
    /// Remove an agent from the allowed-access list.
    AllowedAgentRemove,
    /// Add a group to the allowed-group list.
    AllowedGroupAdd,
    /// Remove a group from the allowed-group list.
    AllowedGroupRemove,
    /// Add an agent to the ban list.
    BannedAgentAdd,
    /// Remove an agent from the ban list.
    BannedAgentRemove,
    /// Add an estate manager.
    ManagerAdd,
    /// Remove an estate manager.
    ManagerRemove,
}

impl EstateAccessDelta {
    /// The `estateaccessdelta` flag bit for this change (matching the reference
    /// viewer's `ESTATE_ACCESS_*` constants).
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::AllowedAgentAdd => 1 << 2,
            Self::AllowedAgentRemove => 1 << 3,
            Self::AllowedGroupAdd => 1 << 4,
            Self::AllowedGroupRemove => 1 << 5,
            Self::BannedAgentAdd => 1 << 6,
            Self::BannedAgentRemove => 1 << 7,
            Self::ManagerAdd => 1 << 8,
            Self::ManagerRemove => 1 << 9,
        }
    }
}

/// Which estate access list a [`Event::EstateAccessList`](crate::Event::EstateAccessList) carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum EstateAccessKind {
    /// The allowed-agents list.
    AllowedAgents,
    /// The allowed-groups list.
    AllowedGroups,
    /// The banned-agents list.
    BannedAgents,
    /// The estate-managers list.
    Managers,
}

/// An estate's configuration, parsed from an `EstateOwnerMessage`
/// `estateupdateinfo` reply to
/// [`Session::request_estate_info`](crate::Session::request_estate_info).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstateInfo {
    /// The estate name.
    pub estate_name: String,
    /// The estate owner's id.
    pub estate_owner: Uuid,
    /// The estate id.
    pub estate_id: u32,
    /// The raw estate-flags bitfield.
    pub estate_flags: u32,
    /// The sun position (when the estate uses a fixed sun).
    pub sun_position: u32,
    /// The parent estate id.
    pub parent_estate: u32,
    /// The estate covenant's notecard id (`None` if there is no covenant).
    pub covenant_id: Option<Uuid>,
    /// When the covenant last changed (Unix timestamp).
    pub covenant_timestamp: u32,
    /// The estate's abuse-report email address.
    pub abuse_email: String,
}

/// An estate's covenant summary, from an `EstateCovenantReply` in response to
/// [`Session::request_estate_covenant`](crate::Session::request_estate_covenant)
/// (`EstateCovenantRequest`). The covenant text itself is an asset fetched
/// separately via the notecard [`covenant_id`](EstateCovenant::covenant_id).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EstateCovenant {
    /// The covenant notecard's asset id (`None` if the estate has no covenant).
    pub covenant_id: Option<Uuid>,
    /// When the covenant last changed (Unix timestamp).
    pub covenant_timestamp: u32,
    /// The estate name.
    pub estate_name: String,
    /// The estate owner's id.
    pub estate_owner_id: Uuid,
}

/// A region's telehub configuration, from a `TelehubInfo` reply to
/// [`Session::request_telehub_info`](crate::Session::request_telehub_info)
/// (and after each telehub-management command). A telehub routes incoming
/// teleports to one of its spawn points.
#[derive(Debug, Clone, PartialEq)]
pub struct TelehubInfo {
    /// The telehub object's id (`None` when the region has no telehub).
    pub object_id: Option<ObjectKey>,
    /// The telehub object's name (empty when there is no telehub).
    pub object_name: String,
    /// The telehub object's region-local position (a fallback the viewer uses
    /// when it cannot find the object itself).
    pub position: Vector,
    /// The telehub object's rotation.
    pub rotation: Rotation,
    /// The spawn points, each relative to the telehub position. Incoming
    /// teleports are routed to one of these.
    pub spawn_points: Vec<Vector>,
}

/// The settings to apply to a region via
/// [`Session::set_region_info`](crate::Session::set_region_info)
/// (`EstateOwnerMessage` method `setregioninfo`). Start from
/// [`RegionInfoUpdate::default`] and set the fields to change.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a distinct region toggle in the setregioninfo wire message"
)]
pub struct RegionInfoUpdate {
    /// Block terraforming by non-owners.
    pub block_terraform: bool,
    /// Block flying.
    pub block_fly: bool,
    /// Allow damage (enable combat).
    pub allow_damage: bool,
    /// Allow residents to resell land.
    pub allow_land_resell: bool,
    /// The maximum concurrent agents.
    pub agent_limit: i32,
    /// The object (prim) bonus multiplier.
    pub object_bonus: f32,
    /// The region maturity rating.
    pub maturity: Maturity,
    /// Restrict pushing (no-push).
    pub restrict_pushobject: bool,
    /// Allow parcel join/subdivide by owners.
    pub allow_parcel_changes: bool,
}

impl Default for RegionInfoUpdate {
    fn default() -> Self {
        Self {
            block_terraform: false,
            block_fly: false,
            allow_damage: false,
            allow_land_resell: true,
            agent_limit: 40,
            object_bonus: 1.0,
            maturity: Maturity::Pg,
            restrict_pushobject: false,
            allow_parcel_changes: true,
        }
    }
}

/// What to do to a user being removed from the agent's land via
/// [`Session::eject_user`](crate::Session::eject_user) (`EjectUser`). The wire
/// `Flags` field is `0` for a plain eject and `0x1` to also add the user to the
/// parcel ban list, matching the reference viewer's `handleEjectAvatar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EjectAction {
    /// Eject the user from the land (send them away).
    Eject,
    /// Eject the user *and* add them to the parcel ban list.
    EjectAndBan,
}

impl EjectAction {
    /// The `EjectUser` `Flags` wire value for this action.
    #[must_use]
    pub const fn to_wire(self) -> u32 {
        match self {
            Self::Eject => 0x0,
            Self::EjectAndBan => 0x1,
        }
    }

    /// The action for an `EjectUser` `Flags` wire value, or `None` if `flags` is
    /// not a recognised eject flag. The inverse of [`EjectAction::to_wire`].
    #[must_use]
    pub const fn from_wire(flags: u32) -> Option<Self> {
        match flags {
            0x0 => Some(Self::Eject),
            0x1 => Some(Self::EjectAndBan),
            _ => None,
        }
    }
}

/// Whether to freeze or unfreeze a user on the agent's land via
/// [`Session::freeze_user`](crate::Session::freeze_user) (`FreezeUser`). The
/// wire `Flags` field is `0` to freeze and `0x1` to unfreeze, matching the
/// reference viewer's `handleFreezeAvatar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeAction {
    /// Freeze the user (prevent them from moving or acting).
    Freeze,
    /// Unfreeze the user.
    Unfreeze,
}

impl FreezeAction {
    /// The `FreezeUser` `Flags` wire value for this action.
    #[must_use]
    pub const fn to_wire(self) -> u32 {
        match self {
            Self::Freeze => 0x0,
            Self::Unfreeze => 0x1,
        }
    }

    /// The action for a `FreezeUser` `Flags` wire value, or `None` if `flags` is
    /// not a recognised freeze flag. The inverse of [`FreezeAction::to_wire`].
    #[must_use]
    pub const fn from_wire(flags: u32) -> Option<Self> {
        match flags {
            0x0 => Some(Self::Freeze),
            0x1 => Some(Self::Unfreeze),
            _ => None,
        }
    }
}

/// Which objects a sim-wide delete targets, applied via
/// [`Session::sim_wide_deletes`](crate::Session::sim_wide_deletes)
/// (`SimWideDeletes`; needs estate/god rights). The wire `Flags` field is the
/// `SWD_*` bitfield from the reference viewer; an all-`false` value (the
/// [`Default`]) deletes every object owned by the target across the region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SimWideDeleteFlags {
    /// Only delete the target's objects on land they do *not* own
    /// (`SWD_OTHERS_LAND_ONLY`).
    pub others_land_only: bool,
    /// Return the objects to their owner instead of deleting them outright
    /// (`SWD_ALWAYS_RETURN_OBJECTS`).
    pub always_return_objects: bool,
    /// Only delete scripted objects (`SWD_SCRIPTED_ONLY`).
    pub scripted_only: bool,
}

impl SimWideDeleteFlags {
    /// The `SimWideDeletes` `Flags` bitfield for this selection.
    #[must_use]
    pub const fn to_wire(self) -> u32 {
        let mut flags = 0_u32;
        if self.others_land_only {
            flags |= 0x1;
        }
        if self.always_return_objects {
            flags |= 0x2;
        }
        if self.scripted_only {
            flags |= 0x4;
        }
        flags
    }

    /// The selection for a `SimWideDeletes` `Flags` bitfield, or `None` if any
    /// bit outside the recognised `SWD_*` set (`0x1` / `0x2` / `0x4`) is set.
    /// The inverse of [`SimWideDeleteFlags::to_wire`].
    #[must_use]
    pub const fn from_wire(flags: u32) -> Option<Self> {
        if flags & !0x7 != 0 {
            return None;
        }
        Some(Self {
            others_land_only: flags & 0x1 != 0,
            always_return_objects: flags & 0x2 != 0,
            scripted_only: flags & 0x4 != 0,
        })
    }
}

/// The region parameters to push with god powers via
/// [`Session::god_update_region_info`](crate::Session::god_update_region_info)
/// (`GodUpdateRegionInfo`; needs grid-god rights). Mirrors the god-tools
/// region floater: the simulator overwrites these fields wholesale, so all of
/// them are sent on every update.
#[derive(Debug, Clone, PartialEq)]
pub struct GodRegionUpdate {
    /// The region (simulator) name. The reference viewer echoes the region's
    /// current name; the simulator can rename the region from this field.
    pub sim_name: RegionName,
    /// The estate this region belongs to.
    pub estate_id: u32,
    /// The parent estate (the "mainland" estate is `1`).
    pub parent_estate_id: u32,
    /// The 64-bit `RegionFlagsExtended` bitfield (build it with
    /// [`sl_wire::RegionFlags`]). The legacy 32-bit `RegionFlags` block is sent
    /// as the low 32 bits, exactly as the reference viewer truncates it.
    pub region_flags: u64,
    /// The billing factor applied to land tier in this region.
    pub billable_factor: f32,
    /// The price per square metre of land, in L$.
    pub price_per_meter: i32,
    /// The grid coordinates teleports into this region are redirected to
    /// (`(0, 0)` for no redirect).
    pub redirect_grid: GridCoordinates,
}

/// A region reported by the world map (one `MapBlockReply` `Data` entry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapRegionInfo {
    /// The region name, or `None` when the grid sent an empty (unknown) name.
    pub name: Option<RegionName>,
    /// The region's grid coordinates (region index pair).
    pub grid_coordinates: GridCoordinates,
    /// The region handle (derived from the grid coordinates).
    pub region_handle: RegionHandle,
    /// The maturity rating, from the map's access byte.
    pub maturity: Maturity,
    /// The raw region flags bitfield.
    pub region_flags: u32,
    /// The region width in metres (256 for standard regions; larger for
    /// variable-sized OpenSim regions).
    pub size_x: u32,
    /// The region height in metres.
    pub size_y: u32,
    /// The number of agents the map reports in the region (often 0).
    pub agents: u8,
    /// The region's water height, in metres (`WaterHeight`; default 20 on most
    /// regions).
    pub water_height: u8,
    /// The region's map tile image id.
    pub map_image_id: TextureKey,
}

/// A kind of world-map overlay item requested via `MapItemRequest` (the
/// `GridItemType`). [`MapItemType::AgentLocations`] gives the avatar "green
/// dots"; the land-for-sale and event types give the corresponding map overlays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MapItemType {
    /// The region's telehub, if any (`1`).
    Telehub,
    /// PG-rated events (`2`).
    PgEvent,
    /// Mature-rated events (`3`).
    MatureEvent,
    /// Avatar locations — the map's "green dots" (`6`).
    AgentLocations,
    /// Parcels for sale, non-adult (`7`).
    LandForSale,
    /// Classified ads (`8`).
    Classified,
    /// Adult-rated events (`9`).
    AdultEvent,
    /// Parcels for sale in adult regions (`10`).
    AdultLandForSale,
    /// Any other grid item type, preserved verbatim.
    Other(u32),
}

impl MapItemType {
    /// Classifies a `GridItemType` wire value.
    #[must_use]
    pub const fn from_u32(value: u32) -> Self {
        match value {
            1 => Self::Telehub,
            2 => Self::PgEvent,
            3 => Self::MatureEvent,
            6 => Self::AgentLocations,
            7 => Self::LandForSale,
            8 => Self::Classified,
            9 => Self::AdultEvent,
            10 => Self::AdultLandForSale,
            other => Self::Other(other),
        }
    }

    /// The wire value for this item type.
    #[must_use]
    pub const fn to_u32(self) -> u32 {
        match self {
            Self::Telehub => 1,
            Self::PgEvent => 2,
            Self::MatureEvent => 3,
            Self::AgentLocations => 6,
            Self::LandForSale => 7,
            Self::Classified => 8,
            Self::AdultEvent => 9,
            Self::AdultLandForSale => 10,
            Self::Other(value) => value,
        }
    }
}

/// A single world-map overlay item from a `MapItemReply`. Coordinates are
/// **global** metres (region origin plus the in-region offset).
///
/// The meaning of `extra`/`extra2` depends on the item's [`MapItemType`]:
/// - [`MapItemType::AgentLocations`]: `extra` is the avatar count at this spot.
/// - [`MapItemType::Telehub`]: `extra2` is `0` for a hub, `1` for an infohub.
/// - [`MapItemType::LandForSale`] / [`MapItemType::AdultLandForSale`]: `extra` is
///   the parcel area in m², `extra2` the sale price in L$.
/// - event types: `extra` is the event id, `extra2` packs the event flags.
// Not `Eq`: `position` ([`GlobalCoordinates`]) holds `f64` components.
#[derive(Debug, Clone, PartialEq)]
pub struct MapItem {
    /// The item's global position in metres (the wire carries integer metres;
    /// the altitude component is unused — the map is 2-D — and is `0`).
    pub position: GlobalCoordinates,
    /// The item's identifier (a parcel/event id, or `None` for avatar dots).
    pub id: Option<Uuid>,
    /// Type-specific context (count, area, event id — see [`MapItem`]).
    pub extra: i32,
    /// Type-specific context (sale price, hub kind, flags — see [`MapItem`]).
    pub extra2: i32,
    /// The item's name (region/parcel/event name, or a hash for avatar dots).
    pub name: String,
}

impl MapItem {
    /// The handle of the region this item sits in, derived from its global
    /// position by [splitting](GlobalCoordinates::split) off the in-region
    /// offset (the typed replacement for masking the 256 m region boundary).
    /// `None` only if the global position lies outside the representable grid,
    /// which never happens for a position the grid actually sent.
    #[must_use]
    pub fn region_handle(&self) -> Option<RegionHandle> {
        Some(RegionHandle::from(self.position.split()?.0))
    }

    /// The item's position within its region (0–256 metres on each axis),
    /// derived from its global position. `None` under the same out-of-grid
    /// condition as [`region_handle`](Self::region_handle).
    #[must_use]
    pub fn region_position(&self) -> Option<RegionCoordinates> {
        Some(self.position.split()?.1)
    }
}

/// One world-map image-tile layer from a `MapLayerReply` (`LayerData` block):
/// the texture covering a rectangular run of regions on the grid. The world map
/// stitches these tiles into the zoomed-out map; `MapBlockReply` then fills in
/// the per-region names and details ([`MapRegionInfo`]).
///
/// The rectangle bounds are **inclusive grid coordinates** (region indices):
/// the tile covers the regions in [`rect`](Self::rect). Second Life's main grid
/// is a single global layer (lower-left `(0, 0)`, upper-right very large — which
/// is why [`GridRectangle`] stores `u32`); OpenSim grids report their own
/// coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLayer {
    /// The inclusive grid-coordinate rectangle the tile covers.
    pub rect: GridRectangle,
    /// The map-tile texture id for this layer.
    pub image_id: TextureKey,
}

/// The `Flags` bitfield the viewer sends in the agent block of the world-map
/// request messages (`MapBlockRequest`, `MapNameRequest`, `MapItemRequest`,
/// `MapLayerRequest`) and which the simulator echoes back in the matching reply.
/// Mirrors the reference viewer's `LLWorldMapMessage` flag constants
/// (`indra/newview/llworldmapmessage.cpp`): a named type in place of the bare
/// `2` "map-layer" magic int. Surfaced by the server-side
/// [`ServerEvent`](crate::ServerEvent) map-request events and consumed by the
/// `SimSession::send_map_*_reply` helpers.
///
/// The bits are independent; an all-clear value (`MapRequestFlags(0)`) is what
/// `MapBlockRequest` carries when it does not select the layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapRequestFlags(pub u32);

impl MapRequestFlags {
    /// Request the world-map image layer (the viewer's `LAYER_FLAG`). Sent on
    /// the name/item/layer requests to select the terrain map tiles.
    pub const LAYER: u32 = 2;
    /// Ask the simulator to also report non-existent ("null") regions in a
    /// `MapBlockReply` (the viewer's `MAP_SIM_RETURN_NULL_SIMS`, used when
    /// probing whether a region exists). Overwrites [`LAYER`](Self::LAYER) on a
    /// `MapBlockRequest`.
    pub const RETURN_NULL_SIMS: u32 = 0x0001_0000;

    /// Whether all of the bits in `mask` are set.
    #[must_use]
    pub const fn contains(self, mask: u32) -> bool {
        self.0 & mask == mask
    }
}

/// A neighbouring simulator announced via `EnableSimulator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborInfo {
    /// The neighbour's region handle.
    pub region_handle: RegionHandle,
    /// The neighbour's UDP address.
    pub sim: SocketAddr,
    /// The neighbour's grid coordinates (region index pair, i.e. global metres
    /// / 256), derived from [`Self::region_handle`].
    pub grid_coordinates: GridCoordinates,
}

#[cfg(test)]
mod tests {
    use super::{MapRequestFlags, Vector};
    use pretty_assertions::assert_eq;
    use sl_types::map::{GridCoordinates, RegionCoordinates};
    use sl_wire::RegionHandle;

    #[test]
    fn map_request_flag_constants_match_the_viewer() {
        // The two named bits the reference viewer (`llworldmapmessage.cpp`)
        // ever sets, by their exact wire values.
        assert_eq!(MapRequestFlags::LAYER, 2);
        assert_eq!(MapRequestFlags::RETURN_NULL_SIMS, 0x0001_0000);
    }

    #[test]
    fn map_request_flags_round_trip_bit_identically() {
        // Wrapping a raw word and reading `.0` back is the identity, so the
        // codec boundary stays byte-identical to the old raw `u32`.
        for raw in [0, MapRequestFlags::LAYER, MapRequestFlags::RETURN_NULL_SIMS] {
            assert_eq!(MapRequestFlags(raw).0, raw);
        }
    }

    #[test]
    fn map_request_flags_contains_checks_the_mask() {
        let layer = MapRequestFlags(MapRequestFlags::LAYER);
        assert!(layer.contains(MapRequestFlags::LAYER));
        assert!(!layer.contains(MapRequestFlags::RETURN_NULL_SIMS));

        let both = MapRequestFlags(MapRequestFlags::LAYER | MapRequestFlags::RETURN_NULL_SIMS);
        assert!(both.contains(MapRequestFlags::LAYER));
        assert!(both.contains(MapRequestFlags::RETURN_NULL_SIMS));

        // An all-clear value (what `MapBlockRequest` carries) contains nothing.
        assert!(!MapRequestFlags(0).contains(MapRequestFlags::LAYER));
    }

    #[test]
    fn map_region_grid_coordinates_match_their_handle() {
        // The typed grid coordinates and the region handle are mutually
        // consistent: the handle is the typed inverse of the coordinates, and
        // decoding the handle's grid index reproduces the original `u16` pair.
        let grid_coordinates = GridCoordinates::new(1000, 1001);
        let region_handle = RegionHandle::from(grid_coordinates);
        assert_eq!(grid_coordinates.x(), 1000);
        assert_eq!(grid_coordinates.y(), 1001);
        assert_eq!(GridCoordinates::from(region_handle), grid_coordinates);
    }

    #[test]
    fn region_coordinates_round_trip_through_a_vector() {
        // The teleport codec boundary unwraps the typed region-local position
        // into the plain wire `Vector` and back; every component survives
        // bit-identically (the same f32 values, in the same order).
        let position = RegionCoordinates::new(128.5, 64.25, 30.0);
        let wire = Vector {
            x: position.x(),
            y: position.y(),
            z: position.z(),
        };
        assert_eq!(RegionCoordinates::from(wire), position);
    }
}
