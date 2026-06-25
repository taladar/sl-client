//! OpenSim extended region settings (`OpenRegionInfo`).
//!
//! `OpenRegionInfo` is an OpenSim-specific CAPS event-queue push: a bag of
//! per-region limits and client-behaviour hints that go beyond the standard
//! Second Life protocol (prim/link/scale limits, build bounds, chat ranges, a
//! UTC offset, …). Second Life never sends it; OpenSim grids use it to tell a
//! viewer about the region's configured limits.
//!
//! Every field is optional: the simulator sends only the keys it wants to
//! override, and the reference viewer applies each independently (Firestorm
//! `indra/newview/llpanelopenregionsettings.cpp`, `OpenRegionInfoUpdate`). An
//! absent key is `None` here; the consumer keeps its previous value, as the
//! viewer does. It surfaces as a typed [`Event`](super::Event) instead of being
//! dropped to a `Diagnostic::UnknownCapsEvent`.

use sl_types::map::RegionCoordinates;

/// OpenSim's per-region limits and client-behaviour overrides
/// (`OpenRegionInfo`). All fields are optional; field names mirror the wire
/// keys parsed by the reference viewer.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenRegionInfo {
    /// Whether the region permits minimap rendering (`AllowMinimap`).
    pub allow_minimap: Option<bool>,
    /// Whether the region permits physical prims (`AllowPhysicalPrims`).
    pub allow_physical_prims: Option<bool>,
    /// The region's maximum draw distance, in metres (`DrawDistance`).
    pub draw_distance: Option<f32>,
    /// Whether the draw distance above is locked (the viewer may not raise it
    /// past [`draw_distance`](Self::draw_distance)) (`ForceDrawDistance`).
    pub force_draw_distance: Option<bool>,
    /// The terrain detail-texture scale factor (`TerrainDetailScale`).
    pub terrain_detail_scale: Option<f32>,
    /// The maximum distance an object may be dragged in one edit, in metres
    /// (`MaxDragDistance`).
    pub max_drag_distance: Option<f32>,
    /// The minimum permitted prim hollow size (`MinHoleSize`).
    pub min_hole_size: Option<f32>,
    /// The maximum permitted prim hollow size (`MaxHollowSize`).
    pub max_hollow_size: Option<f32>,
    /// The maximum number of inventory items transferable in one operation
    /// (`MaxInventoryItemsTransfer`).
    pub max_inventory_items_transfer: Option<i32>,
    /// The maximum number of prims in a linkset (`MaxLinkCount`).
    pub max_link_count: Option<i32>,
    /// The maximum number of prims in a *physical* linkset (`MaxLinkCountPhys`).
    pub max_link_count_phys: Option<i32>,
    /// The upper bound on an object's position within the region, from the
    /// `MaxPosX`/`MaxPosY`/`MaxPosZ` keys.
    pub max_position: Option<RegionCoordinates>,
    /// The lower bound on an object's position within the region, from the
    /// `MinPosX`/`MinPosY`/`MinPosZ` keys.
    pub min_position: Option<RegionCoordinates>,
    /// The maximum size of any single prim dimension (`MaxPrimScale`).
    pub max_prim_scale: Option<f32>,
    /// The maximum size of any single *physical* prim dimension
    /// (`MaxPhysPrimScale`).
    pub max_phys_prim_scale: Option<f32>,
    /// The minimum size of any single prim dimension (`MinPrimScale`).
    pub min_prim_scale: Option<f32>,
    /// The region's offset from UTC, in hours (`OffsetOfUTC`).
    pub offset_of_utc: Option<i32>,
    /// Whether daylight-saving time is in effect for the UTC offset above
    /// (`OffsetOfUTCDST`).
    pub offset_of_utc_dst: Option<bool>,
    /// Whether the region permits water rendering (`RenderWater`).
    pub render_water: Option<bool>,
    /// The `say` chat range, in metres (`SayDistance`).
    pub say_distance: Option<f32>,
    /// The `shout` chat range, in metres (`ShoutDistance`).
    pub shout_distance: Option<f32>,
    /// The `whisper` chat range, in metres (`WhisperDistance`).
    pub whisper_distance: Option<f32>,
    /// Whether teen-mode restrictions are enabled (`ToggleTeenMode`).
    pub teen_mode: Option<bool>,
    /// The avatar name-tag display mode (`ShowTags`).
    pub show_tags: Option<i32>,
    /// Whether the region enforces a maximum build height (`EnforceMaxBuild`).
    pub enforce_max_build: Option<bool>,
    /// The maximum number of groups an avatar may join (`MaxGroups`).
    pub max_groups: Option<i32>,
    /// Whether per-parcel windlight overrides are permitted
    /// (`AllowParcelWindLight`).
    pub allow_parcel_windlight: Option<bool>,
}
