//! Estates, region info updates, and world-map items.

use std::net::SocketAddr;

use super::Maturity;
use sl_types::key::{ObjectKey, TextureKey};
use sl_types::lsl::Rotation;
use sl_types::lsl::Vector;
use sl_types::map::{GridCoordinates, RegionName};
use sl_wire::RegionHandle;
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
    /// The estate covenant's notecard id (nil if none).
    pub covenant_id: Uuid,
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
    /// The covenant notecard's asset id (nil if the estate has no covenant).
    pub covenant_id: Uuid,
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
    /// The telehub object's id (nil when the region has no telehub).
    pub object_id: ObjectKey,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapItem {
    /// The item's global x coordinate in metres.
    pub global_x: u32,
    /// The item's global y coordinate in metres.
    pub global_y: u32,
    /// The item's identifier (a parcel/event id, or nil for avatar dots).
    pub id: Uuid,
    /// Type-specific context (count, area, event id — see [`MapItem`]).
    pub extra: i32,
    /// Type-specific context (sale price, hub kind, flags — see [`MapItem`]).
    pub extra2: i32,
    /// The item's name (region/parcel/event name, or a hash for avatar dots).
    pub name: String,
}

impl MapItem {
    /// The handle of the region this item sits in, derived from its global
    /// coordinates (the global position with the in-region offset masked off).
    #[must_use]
    pub fn region_handle(&self) -> RegionHandle {
        RegionHandle::from_global(self.global_x & !0xFF, self.global_y & !0xFF)
    }

    /// The item's x offset within its region (0–255 metres).
    #[must_use]
    pub const fn local_x(&self) -> u32 {
        self.global_x & 0xFF
    }

    /// The item's y offset within its region (0–255 metres).
    #[must_use]
    pub const fn local_y(&self) -> u32 {
        self.global_y & 0xFF
    }
}

/// One world-map image-tile layer from a `MapLayerReply` (`LayerData` block):
/// the texture covering a rectangular run of regions on the grid. The world map
/// stitches these tiles into the zoomed-out map; `MapBlockReply` then fills in
/// the per-region names and details ([`MapRegionInfo`]).
///
/// The rectangle bounds are **inclusive grid coordinates** (region indices):
/// the tile covers regions `left..=right` by `bottom..=top`. Second Life's main
/// grid is a single global layer (`left = bottom = 0`, `right = top` very large);
/// OpenSim grids report their own coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapLayer {
    /// The left (minimum x) grid coordinate the tile covers, inclusive.
    pub left: u32,
    /// The right (maximum x) grid coordinate the tile covers, inclusive.
    pub right: u32,
    /// The top (maximum y) grid coordinate the tile covers, inclusive.
    pub top: u32,
    /// The bottom (minimum y) grid coordinate the tile covers, inclusive.
    pub bottom: u32,
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
        assert_eq!(
            GridCoordinates::try_from(region_handle),
            Ok(grid_coordinates)
        );
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
