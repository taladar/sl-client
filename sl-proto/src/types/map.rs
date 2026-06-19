//! Estates, region info updates, and world-map items.

use std::net::SocketAddr;

use super::Maturity;
use uuid::Uuid;

/// A change to one of an estate's access lists, applied via
/// [`Session::update_estate_access`](crate::Session::update_estate_access)
/// (`EstateOwnerMessage` method `estateaccessdelta`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The region name.
    pub name: String,
    /// The region's grid x coordinate (region index).
    pub grid_x: u32,
    /// The region's grid y coordinate (region index).
    pub grid_y: u32,
    /// The region handle (derived from the grid coordinates).
    pub region_handle: u64,
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
    pub map_image_id: Uuid,
}

/// A kind of world-map overlay item requested via `MapItemRequest` (the
/// `GridItemType`). [`MapItemType::AgentLocations`] gives the avatar "green
/// dots"; the land-for-sale and event types give the corresponding map overlays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn region_handle(&self) -> u64 {
        let region_x = u64::from(self.global_x & !0xFF);
        let region_y = u64::from(self.global_y & !0xFF);
        (region_x << 32) | region_y
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

/// A neighbouring simulator announced via `EnableSimulator`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborInfo {
    /// The neighbour's region handle.
    pub region_handle: u64,
    /// The neighbour's UDP address.
    pub sim: SocketAddr,
    /// The neighbour's grid x coordinate (region index, i.e. global metres / 256).
    pub grid_x: u32,
    /// The neighbour's grid y coordinate (region index, i.e. global metres / 256).
    pub grid_y: u32,
}
